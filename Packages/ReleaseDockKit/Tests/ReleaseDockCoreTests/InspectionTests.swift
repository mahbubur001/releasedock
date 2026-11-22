import XCTest
@testable import ReleaseDockCore

final class PathSafetyTests: XCTestCase {
    func testRejectsUnsafePaths() {
        let bad: [(String, PathSafety.Violation)] = [
            ("", .empty),
            ("/etc/passwd", .absolutePath),
            ("~/x", .absolutePath),
            ("C:/Windows", .absolutePath),
            ("a/../../b", .parentTraversal),
            ("..", .parentTraversal),
            ("a\\b", .backslash),
            ("a\u{0}b", .nulOrControlCharacter),
            ("a\nb", .nulOrControlCharacter),
            (String(repeating: "x", count: 256), .componentTooLong(String(repeating: "x", count: 256))),
        ]
        for (path, violation) in bad {
            XCTAssertThrowsError(try PathSafety.validateRelativePath(path), path) { error in
                XCTAssertEqual(error as? PathSafety.Violation, violation, path)
            }
        }
    }

    func testNormalizesSafePaths() throws {
        XCTAssertEqual(try PathSafety.validateRelativePath("a/./b//c.php"), ["a", "b", "c.php"])
        XCTAssertEqual(try PathSafety.validateRelativePath("logo@2x.png"), ["logo@2x.png"])
    }

    func testContainmentResolvesSymlinks() throws {
        let dir = TempDir()
        dir.mkdir("root")
        dir.mkdir("outside")
        try FileManager.default.createSymbolicLink(at: dir.path("root/link"), withDestinationURL: dir.path("outside"))
        XCTAssertTrue(PathSafety.isContained(dir.path("root/a/b"), in: dir.path("root")))
        XCTAssertFalse(PathSafety.isContained(dir.path("root/link/x"), in: dir.path("root")))
        XCTAssertFalse(PathSafety.isContained(dir.path("rootless"), in: dir.path("root")))
    }

    func testSlugs() {
        XCTAssertTrue(PathSafety.isValidSlug("my-plugin-2"))
        XCTAssertFalse(PathSafety.isValidSlug("My-Plugin"))
        XCTAssertFalse(PathSafety.isValidSlug("-x"))
        XCTAssertFalse(PathSafety.isValidSlug("a/b"))
        XCTAssertFalse(PathSafety.isValidSlug("../x"))
    }

    func testRedactor() {
        XCTAssertEqual(SecretRedactor.redact("svn --password hunter2 x"), "svn --password <redacted> x")
        XCTAssertEqual(SecretRedactor.redact("https://bob:secret@example.com/x"), "https://bob:<redacted>@example.com/x")
        XCTAssertFalse(SecretRedactor.redact("token ghp_abcdefghijklmnopqrstuvwxyz0123").contains("abcdefghijklmnop"))
        XCTAssertEqual(SecretRedactor.redact("Authorization: Bearer abc.def"), "Authorization: Bearer <redacted>")
    }
}

final class ZIPInspectionTests: XCTestCase {
    let service = ZIPInspectionService()

    private func inspect(_ builder: TestZIPBuilder, options: InspectionOptions = InspectionOptions()) throws -> PackageInspection {
        let dir = TempDir()
        let url = dir.path("p.zip")
        try builder.write(to: url)
        return try service.inspect(zipAt: url, options: options)
    }

    private func samplePlugin() -> TestZIPBuilder {
        var zip = TestZIPBuilder()
        zip.addDirectory("sample-plugin/")
        zip.add("sample-plugin/sample-plugin.php", Fixtures.pluginMainFile(version: "1.0.0"))
        zip.add("sample-plugin/readme.txt", Fixtures.pluginReadme(stableTag: "1.0.0"))
        return zip
    }

    func testCleanArchive() throws {
        let result = try inspect(samplePlugin())
        XCTAssertEqual(result.archiveRootName, "sample-plugin")
        XCTAssertTrue(result.blockingIssues.isEmpty, "\(result.blockingIssues)")
        XCTAssertEqual(Set(result.includedFiles.map(\.path)), ["sample-plugin.php", "readme.txt"])
        XCTAssertEqual(result.sha256.count, 64)
    }

    func testRealDeflateArchiveExtractsWithMatchingHashes() throws {
        let dir = TempDir()
        let plugin = Fixtures.makePlugin(in: dir, version: "1.2.0", extraFiles: [".DS_Store": "junk", "node_modules/x/index.js": "x", "logo@2x.png": String(repeating: "A", count: 5000)])
        let zipURL = dir.path("sample-plugin.zip")
        try Fixtures.zip(folder: plugin, to: zipURL)
        let inspection = try service.inspect(zipAt: zipURL, options: InspectionOptions())
        XCTAssertTrue(inspection.blockingIssues.isEmpty, "\(inspection.blockingIssues)")
        XCTAssertTrue(inspection.excludedPaths.contains(".DS_Store"))
        XCTAssertTrue(inspection.excludedPaths.contains("node_modules/"))
        let destination = dir.path("out")
        let extracted = try service.extract(zipAt: zipURL, inspection: inspection, to: destination)
        for entry in extracted where entry.kind == .file {
            XCTAssertEqual(entry.sha256, try Hashing.sha256(ofFile: plugin.appendingPathComponent(entry.path)), entry.path)
        }
        XCTAssertFalse(FileManager.default.fileExists(atPath: destination.appendingPathComponent("node_modules").path))
        XCTAssertTrue(FileManager.default.fileExists(atPath: destination.appendingPathComponent("logo@2x.png").path))
    }

    func testPathTraversalIsBlocked() throws {
        var zip = samplePlugin()
        zip.add("sample-plugin/../../evil.php", "<?php")
        zip.add("/etc/evil", "x")
        let result = try inspect(zip)
        XCTAssertTrue(result.blockingIssues.contains { $0.kind == .pathTraversal })
        XCTAssertTrue(result.blockingIssues.contains { $0.kind == .absolutePath })
        XCTAssertThrowsError(try service.extract(zipAt: URL(fileURLWithPath: result.sourcePath), inspection: result, to: TempDir().path("x")))
    }

    func testSymlinkIsBlocked() throws {
        var zip = samplePlugin()
        zip.addSymlink("sample-plugin/link.php", target: "/etc/passwd")
        let result = try inspect(zip)
        XCTAssertTrue(result.blockingIssues.contains { $0.kind == .symbolicLink && $0.path == "sample-plugin/link.php" })
    }

    func testDuplicateAndCaseConflictingPaths() throws {
        var zip = samplePlugin()
        zip.add("sample-plugin/README.TXT", "dup")
        zip.add("sample-plugin/readme.txt", "dup2")
        let result = try inspect(zip)
        XCTAssertGreaterThanOrEqual(result.blockingIssues.filter { $0.kind == .duplicatePath }.count, 2)
    }

    func testFileUsedAsDirectoryIsBlocked() throws {
        var zip = samplePlugin()
        zip.add("sample-plugin/inc", "file")
        zip.add("sample-plugin/inc/x.php", "<?php")
        let result = try inspect(zip)
        XCTAssertTrue(result.blockingIssues.contains { $0.kind == .duplicatePath })
    }

    func testNestedRootIsBlocked() throws {
        var zip = TestZIPBuilder()
        zip.add("sample-plugin/sample-plugin/sample-plugin.php", Fixtures.pluginMainFile(version: "1.0.0"))
        let result = try inspect(zip)
        XCTAssertTrue(result.blockingIssues.contains { $0.kind == .nestedRoot })
    }

    func testMacOSJunkAndDevFilesAreExcluded() throws {
        var zip = samplePlugin()
        zip.add("__MACOSX/sample-plugin/._sample-plugin.php", "junk")
        zip.add("sample-plugin/.DS_Store", "junk")
        zip.add("sample-plugin/.git/config", "[core]")
        zip.add("sample-plugin/.github/workflows/ci.yml", "x")
        zip.add("sample-plugin/empty.php", "")
        let result = try inspect(zip)
        XCTAssertEqual(result.archiveRootName, "sample-plugin", "__MACOSX must not break root detection")
        XCTAssertTrue(result.blockingIssues.isEmpty, "\(result.blockingIssues)")
        XCTAssertEqual(Set(result.excludedPaths), ["__MACOSX/", ".DS_Store", ".git/", ".github/"])
        XCTAssertTrue(result.issues.contains { $0.kind == .zeroByteFile && $0.path == "empty.php" })
        XCTAssertFalse(result.includedFiles.contains { $0.path.hasPrefix(".git") })
    }

    func testForbiddenAndSecretFilesBlockZIP() throws {
        var zip = samplePlugin()
        zip.add("sample-plugin/build.zip", "PK")
        zip.add("sample-plugin/.env", "DB_PASSWORD=x")
        let result = try inspect(zip)
        XCTAssertTrue(result.blockingIssues.contains { $0.kind == .forbiddenFile && $0.path == "build.zip" })
        XCTAssertTrue(result.blockingIssues.contains { $0.kind == .possibleSecret && $0.path == ".env" })
    }

    func testControlCharactersAndNonUTF8Names() throws {
        var zip = samplePlugin()
        zip.add("sample-plugin/bad\u{7}name.php", "x")
        var raw = TestZIPBuilder.Entry(name: "sample-plugin/x", data: Data("x".utf8))
        raw.flags = 0
        zip.entries.append(raw)
        let result = try inspect(zip)
        XCTAssertTrue(result.blockingIssues.contains { $0.kind == .unsafeFilename })
    }

    func testEncryptedEntryIsBlocked() throws {
        var zip = samplePlugin()
        var entry = TestZIPBuilder.Entry(name: "sample-plugin/secret.php", data: Data("x".utf8))
        entry.flags = 0x0801
        zip.entries.append(entry)
        let result = try inspect(zip)
        XCTAssertTrue(result.blockingIssues.contains { $0.kind == .encryptedEntry })
    }

    func testLocalHeaderNameMismatchIsCorrupt() throws {
        var zip = samplePlugin()
        var entry = TestZIPBuilder.Entry(name: "sample-plugin/ok.php", data: Data("x".utf8))
        entry.localName = "sample-plugin/../evil.php"
        zip.entries.append(entry)
        let result = try inspect(zip)
        XCTAssertTrue(result.blockingIssues.contains { $0.kind == .corruptArchive })
    }

    func testHugeZIP64OffsetsAreRejectedWithoutCrashing() throws {
        var data = samplePlugin().build()
        // Turn the EOCD into a ZIP64 marker and append a locator pointing near Int.max.
        let eocd = data.count - 22
        data.replaceSubrange((eocd + 16)..<(eocd + 20), with: Data([0xFF, 0xFF, 0xFF, 0xFF]))
        var locator = Data([0x50, 0x4B, 0x06, 0x07, 0, 0, 0, 0])
        locator += withUnsafeBytes(of: UInt64(Int.max - 2).littleEndian) { Data($0) }
        locator += Data([1, 0, 0, 0])
        data.insert(contentsOf: locator, at: eocd)
        let dir = TempDir()
        let url = dir.path("evil.zip")
        try data.write(to: url)
        let result = try service.inspect(zipAt: url, options: InspectionOptions())
        XCTAssertTrue(result.blockingIssues.contains { $0.kind == .corruptArchive })
    }

    func testInvalidZIP() throws {
        let dir = TempDir()
        let url = dir.write("broken.zip", "this is not a zip archive at all")
        let result = try service.inspect(zipAt: url, options: InspectionOptions())
        XCTAssertTrue(result.blockingIssues.contains { $0.kind == .corruptArchive })
    }

    func testSizeLimits() throws {
        var zip = samplePlugin()
        zip.add("sample-plugin/big.txt", String(repeating: "a", count: 4096))
        let result = try inspect(zip, options: InspectionOptions(maximumTotalSize: 1024, maximumFileSize: 2048))
        XCTAssertTrue(result.blockingIssues.contains { $0.kind == .archiveTooLarge })
    }

    func testExtractionRefusesChangedArchive() throws {
        let dir = TempDir()
        let url = dir.path("p.zip")
        try samplePlugin().write(to: url)
        let inspection = try service.inspect(zipAt: url, options: InspectionOptions())
        var changed = samplePlugin()
        changed.add("sample-plugin/new.php", "<?php")
        try changed.write(to: url)
        XCTAssertThrowsError(try service.extract(zipAt: url, inspection: inspection, to: dir.path("out")))
    }
}

final class FolderInspectionTests: XCTestCase {
    func testFolderInspectionExcludesArtifactsAndHashesFiles() throws {
        let dir = TempDir()
        let root = Fixtures.makePlugin(in: dir, version: "1.0.0", extraFiles: [
            ".git/HEAD": "ref: refs/heads/main",
            ".gitignore": "node_modules",
            "node_modules/pkg/index.js": "x",
            ".env": "SECRET=1",
            "package-lock.json": "{}",
            "debug.log": "x",
            "empty.txt": "",
            "tests/test-x.php": "<?php",
        ])
        let before = try FileManager.default.subpathsOfDirectory(atPath: root.path).sorted()
        let result = try FolderInspectionService().inspect(folderAt: root, options: InspectionOptions(excludePatterns: ["/tests"]))
        let after = try FileManager.default.subpathsOfDirectory(atPath: root.path).sorted()
        XCTAssertEqual(before, after, "inspection must not modify the folder")
        let files = Set(result.includedFiles.map(\.path))
        XCTAssertTrue(files.contains("sample-plugin.php"))
        XCTAssertFalse(files.contains(".env"))
        XCTAssertFalse(files.contains { $0.hasPrefix(".git") || $0.hasPrefix("node_modules") || $0.hasPrefix("tests") })
        XCTAssertFalse(files.contains("package-lock.json"))
        XCTAssertFalse(files.contains("debug.log"))
        XCTAssertTrue(result.issues.contains { $0.kind == .possibleSecret && $0.excluded })
        XCTAssertTrue(result.issues.contains { $0.kind == .zeroByteFile })
        XCTAssertTrue(result.blockingIssues.isEmpty, "\(result.blockingIssues)")
        XCTAssertTrue(result.includedFiles.allSatisfy { $0.sha256 != nil })
    }

    func testSymlinksAreBlocked() throws {
        let dir = TempDir()
        let root = Fixtures.makePlugin(in: dir, version: "1.0.0")
        try FileManager.default.createSymbolicLink(at: root.appendingPathComponent("link.php"), withDestinationURL: URL(fileURLWithPath: "/etc/hosts"))
        let result = try FolderInspectionService().inspect(folderAt: root, options: InspectionOptions())
        XCTAssertTrue(result.blockingIssues.contains { $0.kind == .symbolicLink && $0.path == "link.php" })
    }

    func testDistignoreIsApplied() throws {
        let dir = TempDir()
        let root = Fixtures.makePlugin(in: dir, version: "1.0.0", extraFiles: [".distignore": "# comment\n/src\n*.ts\n", "src/app.ts": "x", "lib/x.ts": "y"])
        let result = try FolderInspectionService().inspect(folderAt: root, options: InspectionOptions())
        let files = Set(result.includedFiles.map(\.path))
        XCTAssertFalse(files.contains("src/app.ts"))
        XCTAssertFalse(files.contains("lib/x.ts"))
        XCTAssertFalse(files.contains(".distignore"))
    }

    func testGlobPatterns() {
        XCTAssertTrue(GlobPattern("*.log")!.matches(path: "a/b/debug.log", isDirectory: false))
        XCTAssertTrue(GlobPattern("/build")!.matches(path: "build", isDirectory: true))
        XCTAssertFalse(GlobPattern("/build")!.matches(path: "src/build", isDirectory: true))
        XCTAssertTrue(GlobPattern("docs/")!.matches(path: "x/docs", isDirectory: true))
        XCTAssertFalse(GlobPattern("docs/")!.matches(path: "docs", isDirectory: false))
        XCTAssertTrue(GlobPattern("**/fixtures")!.matches(path: "tests/data/fixtures", isDirectory: true))
        XCTAssertNil(GlobPattern("!keep.php"))
        XCTAssertNil(GlobPattern("# c"))
    }
}

final class ProductDetectorTests: XCTestCase {
    func testDetectsPlugin() throws {
        let dir = TempDir()
        let root = Fixtures.makePlugin(in: dir, version: "2.3.4")
        let detected = try ProductDetector().detect(at: root)
        XCTAssertEqual(detected.type, .plugin)
        XCTAssertEqual(detected.name, "Sample Plugin")
        XCTAssertEqual(detected.version, "2.3.4")
        XCTAssertEqual(detected.mainFile, "sample-plugin.php")
        XCTAssertEqual(detected.readme?.stableTag, "2.3.4")
    }

    func testAmbiguousMainFile() throws {
        let dir = TempDir()
        let root = Fixtures.makePlugin(in: dir, version: "1.0", extraFiles: ["other.php": Fixtures.pluginMainFile(name: "Other", version: "9.0")])
        XCTAssertEqual(try ProductDetector().detect(at: root, slug: "sample-plugin").mainFile, "sample-plugin.php")
        let ambiguous = try ProductDetector().detect(at: root, slug: "nomatch")
        XCTAssertNil(ambiguous.mainFile)
        XCTAssertFalse(ambiguous.problems.isEmpty)
        XCTAssertEqual(try ProductDetector().detect(at: root, mainPluginFile: "other.php").version, "9.0")
    }

    func testDetectsTheme() throws {
        let dir = TempDir()
        let root = Fixtures.makeTheme(in: dir, version: "1.4")
        let detected = try ProductDetector().detect(at: root)
        XCTAssertEqual(detected.type, .theme)
        XCTAssertEqual(detected.version, "1.4")
        XCTAssertEqual(detected.name, "Sample Theme")
    }

    func testNothingDetected() {
        let dir = TempDir()
        dir.write("x/readme.txt", "hello")
        XCTAssertThrowsError(try ProductDetector().detect(at: dir.path("x"))) { error in
            XCTAssertEqual(error as? ProductDetectionError, .noProductFound)
        }
    }

    func testVersionNeverComesFromFileName() throws {
        let dir = TempDir()
        let plugin = Fixtures.makePlugin(in: dir, at: "sample-plugin", version: "1.0.0")
        let zip = dir.path("sample-plugin-9.9.9.zip")
        try Fixtures.zip(folder: plugin, to: zip)
        let inspection = try ZIPInspectionService().inspect(zipAt: zip, options: InspectionOptions())
        let out = dir.path("x")
        _ = try ZIPInspectionService().extract(zipAt: zip, inspection: inspection, to: out)
        XCTAssertEqual(try ProductDetector().detect(at: out).version, "1.0.0")
    }
}
