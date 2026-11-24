import XCTest
@testable import ReleaseDockCore

final class BuildServiceTests: XCTestCase {
    func testBuildsOnACloneAndLeavesTheSourceUntouched() async throws {
        let dir = TempDir()
        let source = Fixtures.makePlugin(in: dir, at: "dev/sample-plugin", version: "1.1.0", extraFiles: ["src/app.ts": "let a = 1"])
        let before = try FileManager.default.subpathsOfDirectory(atPath: source.path).sorted()
        let result = try await BuildService().build(
            source: source,
            command: "mkdir -p build && echo compiled > build/app.js && rm -rf src && echo done",
            workspace: dir.path("staging")
        )
        XCTAssertEqual(result.exitCode, 0)
        XCTAssertTrue(result.outputTail.contains("done"))
        XCTAssertTrue(result.builtPath.hasSuffix("/build/sample-plugin"), "keeps the folder name")
        let built = URL(fileURLWithPath: result.builtPath)
        XCTAssertEqual(try String(contentsOf: built.appendingPathComponent("build/app.js"), encoding: .utf8), "compiled\n")
        XCTAssertFalse(FileManager.default.fileExists(atPath: built.appendingPathComponent("src").path))
        XCTAssertEqual(try FileManager.default.subpathsOfDirectory(atPath: source.path).sorted(), before, "source untouched")
    }

    func testFailingBuildReportsExitCodeAndOutput() async throws {
        let dir = TempDir()
        let source = Fixtures.makePlugin(in: dir, at: "dev/sample-plugin", version: "1.1.0")
        do {
            _ = try await BuildService().build(source: source, command: "echo 'npm ERR! missing script: build' >&2; exit 3", workspace: dir.path("staging"))
            XCTFail("expected a build failure")
        } catch ReleaseError.buildFailed(let command, let exitCode, let output) {
            XCTAssertEqual(exitCode, 3)
            XCTAssertTrue(command.contains("exit 3"))
            XCTAssertTrue(output.contains("missing script: build"))
        }
    }

    func testRebuildReplacesThePreviousCopy() async throws {
        let dir = TempDir()
        let source = Fixtures.makePlugin(in: dir, at: "dev/sample-plugin", version: "1.1.0")
        let first = try await BuildService().build(source: source, command: "touch stale.txt", workspace: dir.path("staging"))
        let second = try await BuildService().build(source: source, command: "true", workspace: dir.path("staging"))
        XCTAssertEqual(first.builtPath, second.builtPath)
        XCTAssertFalse(FileManager.default.fileExists(atPath: second.builtPath + "/stale.txt"))
    }
}

final class ExportIgnoreTests: XCTestCase {
    func testParsesExportIgnore() {
        let text = """
        # Release exclusions
        /tests export-ignore
        *.md   export-ignore
        /.github export-ignore
        *.php text eol=lf
        phpunit.xml.dist export-ignore -diff
        """
        XCTAssertEqual(FolderInspectionService.exportIgnorePatterns(text), ["/tests", "*.md", "/.github", "phpunit.xml.dist"])
    }

    func testGitattributesApplyOnlyWithoutDistignore() throws {
        let dir = TempDir()
        let root = Fixtures.makePlugin(in: dir, version: "1.1.0", extraFiles: [
            ".gitattributes": "/tests export-ignore\nCONTRIBUTING.md export-ignore\n",
            "tests/test-a.php": "<?php",
            "CONTRIBUTING.md": "x",
        ])
        var paths = try FolderInspectionService().inspect(folderAt: root, options: InspectionOptions()).includedFiles.map(\.path)
        XCTAssertFalse(paths.contains("tests/test-a.php"))
        XCTAssertFalse(paths.contains("CONTRIBUTING.md"))

        dir.write("sample-plugin/.distignore", "/vendor\n")
        paths = try FolderInspectionService().inspect(folderAt: root, options: InspectionOptions()).includedFiles.map(\.path)
        XCTAssertTrue(paths.contains("tests/test-a.php"), ".distignore takes precedence")
    }
}

final class BuildPipelineTests: XCTestCase {
    func testFolderReleaseShipsTheBuiltCopy() async throws {
        guard LocalSVNRepository.isAvailable else { throw XCTSkip("svn/svnadmin not installed") }
        let repo = try LocalSVNRepository()
        let work = TempDir()
        let store = try Store.inMemory()
        let paths = AppPaths(root: work.path("data"))
        try paths.createAll()
        let pipeline = ReleasePipelineService(dependencies: PipelineDependencies(
            products: store, releases: store, svn: SVNService(commandTimeout: .seconds(60)), git: StubGit(), keychain: InMemoryKeychain(),
            staging: ReleaseStagingService(paths: paths), builder: BuildService(), generator: "tests"
        ))
        try repo.createPlugin(slug: "sample-plugin", initialVersion: "1.0.0")
        let product = Product(
            name: "Sample Plugin", slug: "sample-plugin", type: .plugin,
            svnRepositoryURL: URL(string: "\(repo.url)/sample-plugin")!,
            workingCopyPath: work.path("wc-plugin").path, currentVersion: "1.0.0",
            buildCommand: "mkdir -p build && echo compiled > build/app.js"
        )
        try store.save(product)
        let source = Fixtures.makePlugin(in: work, at: "dev/sample-plugin", version: "1.1.0")

        var record = try await pipeline.createRelease(productID: product.id, source: .folder(path: source.path))
        record = try await pipeline.prepare(record.id)
        XCTAssertEqual(record.state, .previewed, "\(record.validation?.blockingResults ?? []) \(record.errors)")
        XCTAssertEqual(record.build?.exitCode, 0)
        XCTAssertTrue(record.preview!.changes.contains { $0.path == "trunk/build/app.js" })
        XCTAssertFalse(FileManager.default.fileExists(atPath: source.appendingPathComponent("build").path), "dev folder untouched")
        _ = try await pipeline.approve(record.id, kind: .user)
        record = try await pipeline.publish(record.id)
        XCTAssertEqual(record.state, .completed, "\(record.errors)")
        XCTAssertEqual(record.verification?.isClean, true)
        XCTAssertEqual(try repo.run(["cat", "\(repo.url)/sample-plugin/tags/1.1.0/build/app.js"]), "compiled\n")
    }

    func testBuildFailureFailsTheRelease() async throws {
        guard LocalSVNRepository.isAvailable else { throw XCTSkip("svn/svnadmin not installed") }
        let repo = try LocalSVNRepository()
        let work = TempDir()
        let store = try Store.inMemory()
        let paths = AppPaths(root: work.path("data"))
        try paths.createAll()
        let pipeline = ReleasePipelineService(dependencies: PipelineDependencies(
            products: store, releases: store, svn: SVNService(commandTimeout: .seconds(60)), git: StubGit(), keychain: InMemoryKeychain(),
            staging: ReleaseStagingService(paths: paths), builder: BuildService(), generator: "tests"
        ))
        try repo.createPlugin(slug: "sample-plugin", initialVersion: "1.0.0")
        let product = Product(
            name: "Sample Plugin", slug: "sample-plugin", type: .plugin,
            svnRepositoryURL: URL(string: "\(repo.url)/sample-plugin")!,
            workingCopyPath: work.path("wc-plugin").path, currentVersion: "1.0.0",
            buildCommand: "exit 1"
        )
        try store.save(product)
        let source = Fixtures.makePlugin(in: work, at: "dev/sample-plugin", version: "1.1.0")
        let record = try await pipeline.createRelease(productID: product.id, source: .folder(path: source.path))
        await XCTAssertThrowsErrorAsync(try await pipeline.prepare(record.id))
        XCTAssertEqual(try store.release(id: record.id)?.state, .failed)
    }
}
