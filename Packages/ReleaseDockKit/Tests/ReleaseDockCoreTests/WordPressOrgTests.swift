import XCTest
@testable import ReleaseDockCore

final class WordPressOrgTests: XCTestCase {
    func testParsesLatestVersionFromVersionCheck() throws {
        let json = #"{"offers":[{"response":"upgrade","current":"7.1.5","version":"7.1.5"},{"response":"autoupdate","current":"7.0.9"}],"translations":[]}"#
        XCTAssertEqual(try WordPressOrgService.parseLatestVersion(Data(json.utf8)), "7.1.5")
    }

    func testRejectsUnexpectedVersionCheckPayloads() {
        XCTAssertThrowsError(try WordPressOrgService.parseLatestVersion(Data(#"{"offers":[]}"#.utf8)))
        XCTAssertThrowsError(try WordPressOrgService.parseLatestVersion(Data(#"{"offers":[{"current":"latest"}]}"#.utf8)))
        XCTAssertThrowsError(try WordPressOrgService.parseLatestVersion(Data("<html>".utf8)))
    }

    func testBranches() {
        XCTAssertEqual(WordPressBranch.of("7.1.5"), "7.1")
        XCTAssertEqual(WordPressBranch.of("7.1"), "7.1")
        XCTAssertEqual(WordPressBranch.of("7"), "7.0")
        XCTAssertNil(WordPressBranch.of("WP 6.9"))
        XCTAssertEqual(WordPressBranch.compare("6.9", "7.1.5"), -1)
        XCTAssertEqual(WordPressBranch.compare("7.1", "7.1.5"), 0)
        XCTAssertEqual(WordPressBranch.compare("7.1.2", "7.1.5"), 0, "point releases share a branch")
        XCTAssertEqual(WordPressBranch.compare("7.2", "7.1.5"), 1)
        XCTAssertNil(WordPressBranch.compare("soon", "7.1"))
    }
}

final class TestedUpToFreshnessTests: XCTestCase {
    let validator = ValidationService()

    /// Fixture plugins declare "Tested up to: 6.8".
    func report(latest: String?, mainFileHeader: String? = nil) throws -> ValidationReport {
        let dir = TempDir()
        var extra: [String: String] = [:]
        if let mainFileHeader {
            extra["sample-plugin.php"] = Fixtures.pluginMainFile(version: "1.1.0")
                .replacingOccurrences(of: " * Requires PHP: 7.4", with: " * Requires PHP: 7.4\n * Tested up to: \(mainFileHeader)")
        }
        let root = Fixtures.makePlugin(in: dir, version: "1.1.0", extraFiles: extra)
        let inspection = try FolderInspectionService().inspect(folderAt: root, options: InspectionOptions())
        let detected = try ProductDetector().detect(at: root, expectedType: .plugin, slug: "sample-plugin")
        var repo = RepositorySnapshot(repositoryExists: true, currentVersion: "1.0.0", releasePathExists: false, authentication: .keychain)
        repo.latestWordPressVersion = latest
        return validator.validate(ValidationContext(product: makeProduct(), inspection: inspection, detected: detected, repository: repo))
    }

    func tested(_ report: ValidationReport) -> [ValidationResult] { report.results.filter { $0.check == .testedUpTo } }

    func testOutdatedTestedUpToWarnsWithFix() throws {
        let results = tested(try report(latest: "7.1.5"))
        XCTAssertEqual(results.count, 1, "the format-only pass is replaced: \(results)")
        let warning = try XCTUnwrap(results.first)
        XCTAssertEqual(warning.status, .warning)
        XCTAssertTrue(warning.message.contains("6.8") && warning.message.contains("7.1.5"))
        XCTAssertEqual(warning.filePath, "readme.txt")
        XCTAssertEqual(warning.suggestion, "Test with WordPress 7.1, then set \"Tested up to: 7.1\".")
    }

    func testCurrentBranchPasses() throws {
        let results = tested(try report(latest: "6.8.3"))
        XCTAssertEqual(results.map(\.status), [.passed])
        XCTAssertTrue(results[0].message.contains("covers"))
    }

    func testUnknownLatestVersionKeepsFormatCheckOnly() throws {
        let results = tested(try report(latest: nil))
        XCTAssertEqual(results.map(\.status), [.passed])
        XCTAssertEqual(results[0].message, "Tested up to 6.8")
    }

    func testPluginHeaderMismatchWarns() throws {
        let results = tested(try report(latest: "6.8.1", mainFileHeader: "6.7"))
        XCTAssertTrue(results.contains { $0.status == .warning && $0.message.contains("6.7") && $0.filePath == "sample-plugin.php" }, "\(results)")
        XCTAssertFalse(results.contains { $0.status == .passed })
    }

    func testMatchingPluginHeaderDoesNotWarn() throws {
        let results = tested(try report(latest: "6.8.1", mainFileHeader: "6.8"))
        XCTAssertEqual(results.map(\.status), [.passed])
    }

    func testOutdatedTestedUpToNeverBlocks() throws {
        XCTAssertFalse(try report(latest: "7.1.5").isBlocking)
    }

    func testDisabledCheckIsDropped() throws {
        var product = makeProduct()
        product.validation.disabledChecks = [.testedUpTo]
        let dir = TempDir()
        let root = Fixtures.makePlugin(in: dir, version: "1.1.0")
        let inspection = try FolderInspectionService().inspect(folderAt: root, options: InspectionOptions())
        let detected = try ProductDetector().detect(at: root, expectedType: .plugin, slug: "sample-plugin")
        var repo = RepositorySnapshot(repositoryExists: true, releasePathExists: false, authentication: .keychain)
        repo.latestWordPressVersion = "7.1.5"
        let report = validator.validate(ValidationContext(product: product, inspection: inspection, detected: detected, repository: repo))
        XCTAssertTrue(tested(report).isEmpty)
    }
}
