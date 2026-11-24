import XCTest
@testable import ReleaseDockCore

/// Restoring an older version's code as a new, higher version, against local file:// repositories.
final class RestoreReleaseTests: XCTestCase {
    var repo: LocalSVNRepository!
    var work: TempDir!
    var store: Store!
    var pipeline: ReleasePipelineService!

    override func setUp() async throws {
        guard LocalSVNRepository.isAvailable else { throw XCTSkip("svn/svnadmin not installed") }
        repo = try LocalSVNRepository()
        work = TempDir()
        store = try Store.inMemory()
        let paths = AppPaths(root: work.path("data"))
        try paths.createAll()
        pipeline = ReleasePipelineService(dependencies: PipelineDependencies(
            products: store, releases: store, svn: FaultInjectingSVN(base: SVNService(commandTimeout: .seconds(60))),
            git: StubGit(), keychain: InMemoryKeychain(), staging: ReleaseStagingService(paths: paths), generator: "tests"
        ))
    }

    /// Sample Plugin with 1.0.0 released, then 1.1.0 released with an extra (broken) file.
    func pluginWithTwoReleases(requireCleanGitTree: Bool = false) async throws -> Product {
        try repo.createPlugin(slug: "sample-plugin", initialVersion: "1.0.0")
        var product = Product(
            name: "Sample Plugin", slug: "sample-plugin", type: .plugin,
            svnRepositoryURL: URL(string: "\(repo.url)/sample-plugin")!,
            workingCopyPath: work.path("wc-plugin").path,
            currentVersion: "1.0.0", releaseMode: .assisted
        )
        product.validation.requireCleanGitTree = requireCleanGitTree
        try store.save(product)

        let src = TempDir()
        let plugin = Fixtures.makePlugin(in: src, version: "1.1.0", extraFiles: ["includes/broken.php": "<?php\n// broken\n"])
        let zip = work.path("sample-plugin-1.1.0.zip")
        try Fixtures.zip(folder: plugin, to: zip)
        var record = try await pipeline.createRelease(productID: product.id, source: .zip(path: zip.path), mode: .assisted)
        record = try await pipeline.prepare(record.id)
        record = try await pipeline.approve(record.id, kind: .user)
        record = try await pipeline.publish(record.id)
        XCTAssertEqual(record.state, .completed, "\(record.errors)")
        return try XCTUnwrap(try store.product(id: product.id))
    }

    func remote(_ path: String) throws -> String {
        try repo.run(["cat", "\(repo.url)/sample-plugin/\(path)"])
    }

    func testOptionsListOlderVersionsAndSuggestAVersionAboveTheHighest() async throws {
        let product = try await pluginWithTwoReleases()
        let options = try await pipeline.restoreOptions(productID: product.id)
        XCTAssertEqual(options.currentVersion, "1.1.0")
        XCTAssertEqual(options.olderVersions, ["1.0.0"])
        XCTAssertEqual(options.suggestedVersion, "1.1.1")
    }

    func testRestoresOlderCodeAsNewVersionThroughTheNormalRelease() async throws {
        let product = try await pluginWithTwoReleases(requireCleanGitTree: true)
        var record = try await pipeline.createRestoreRelease(productID: product.id, restoring: "1.0.0", as: "1.1.1", changelogNotes: "Restores the 1.0.0 code after a problem in 1.1.0.", mode: .assisted)
        XCTAssertEqual(record.restoresVersion, "1.0.0")
        XCTAssertEqual(record.source.kind, .folder)

        record = try await pipeline.prepare(record.id)
        XCTAssertEqual(record.state, .previewed, "\(record.validation?.blockingResults ?? []) \(record.errors)")
        XCTAssertEqual(record.version, "1.1.1")
        XCTAssertEqual(record.previousVersion, "1.1.0")
        XCTAssertTrue(record.validation?.results.contains { $0.check == .gitWorkingTree && $0.status == .passed } == true, "an exported tag has no Git tree")
        let preview = try XCTUnwrap(record.preview)
        XCTAssertTrue(preview.changes.contains { $0.path == "trunk/includes/broken.php" && $0.kind == .deleted })
        XCTAssertTrue(preview.changes.contains { $0.path == "tags/1.1.1" })
        XCTAssertTrue(preview.commitMessage.hasPrefix("Release Sample Plugin 1.1.1 (restores the 1.0.0 code)"))

        record = try await pipeline.approve(record.id, kind: .user)
        record = try await pipeline.publish(record.id)
        XCTAssertEqual(record.state, .completed, "\(record.errors)")
        XCTAssertEqual(record.verification?.isClean, true, "\(String(describing: record.verification))")

        let readme = try remote("trunk/readme.txt")
        XCTAssertTrue(readme.contains("Stable tag: 1.1.1"))
        XCTAssertTrue(readme.contains("= 1.1.1 - "))
        XCTAssertTrue(readme.contains("* Restores the 1.0.0 code after a problem in 1.1.0."))
        XCTAssertTrue(try remote("tags/1.1.1/sample-plugin.php").contains("Version: 1.1.1"))
        XCTAssertThrowsError(try remote("tags/1.1.1/includes/broken.php"))
        XCTAssertTrue(try remote("tags/1.1.0/includes/broken.php").contains("broken"), "released tags are never changed")
        XCTAssertEqual(try store.product(id: product.id)?.currentVersion, "1.1.1")
    }

    func testRefusesVersionsThatWouldNotUpdateSites() async throws {
        let product = try await pluginWithTwoReleases()
        let cases: [(String, String)] = [
            ("1.1.0", "1.1.1"),  // not older than current
            ("0.5.0", "1.1.1"),  // never released
            ("1.0.0", "1.1.0"),  // not newer than the highest version
            ("1.0.0", "1.0.1"),
        ]
        for (old, new) in cases {
            do {
                _ = try await pipeline.createRestoreRelease(productID: product.id, restoring: old, as: new, changelogNotes: "")
                XCTFail("restoring \(old) as \(new) should be refused")
            } catch ReleaseError.restoreNotPossible {
            } catch {
                XCTFail("unexpected error \(error)")
            }
        }
        XCTAssertEqual(try store.releases(productID: nil).filter { $0.restoresVersion != nil }.count, 0)
    }

    func testRestoresAnOlderThemeVersionIntoANewVersionDirectory() async throws {
        try repo.createTheme(slug: "sample-theme", initialVersion: "1.0.0")
        let src = TempDir()
        let theme = Fixtures.makeTheme(in: src, at: "t", version: "1.1.0")
        src.write("t/broken.php", "<?php\n")
        _ = try repo.run(["import", "-m", "1.1.0", theme.path, "\(repo.url)/sample-theme/1.1.0"])
        let product = Product(
            name: "Sample Theme", slug: "sample-theme", type: .theme,
            svnRepositoryURL: URL(string: "\(repo.url)/sample-theme")!,
            workingCopyPath: work.path("wc-theme").path,
            currentVersion: "1.1.0", releaseMode: .assisted
        )
        try store.save(product)

        let options = try await pipeline.restoreOptions(productID: product.id)
        XCTAssertEqual(options.olderVersions, ["1.0.0"])
        var record = try await pipeline.createRestoreRelease(productID: product.id, restoring: "1.0.0", as: try XCTUnwrap(options.suggestedVersion), changelogNotes: "", mode: .assisted)
        record = try await pipeline.prepare(record.id)
        XCTAssertEqual(record.state, .previewed, "\(record.validation?.blockingResults ?? []) \(record.errors)")
        XCTAssertEqual(record.version, "1.1.1")
        let changes = try XCTUnwrap(record.preview).changes
        XCTAssertTrue(changes.allSatisfy { $0.path.hasPrefix("1.1.1") }, "\(changes.map(\.path))")
        // The new directory is copied from 1.1.0 with history, then the restored code replaces it.
        XCTAssertTrue(changes.contains { $0.path == "1.1.1/broken.php" && $0.kind == .deleted })
    }
}
