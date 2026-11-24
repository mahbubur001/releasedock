import XCTest
@testable import ReleaseDockCore

final class ReadmeRulesTests: XCTestCase {
    func readme(short: String? = "A sample plugin.", description: Bool = true, upgradeNotice: String? = nil, padding: Int = 0) -> WordPressReadme {
        var text = """
        === Sample Plugin ===
        Contributors: testauthor
        Tags: testing
        Tested up to: 7.1
        Stable tag: 1.1.0
        License: GPLv2 or later

        """
        if let short { text += "\n\(short)\n\n" }
        if description { text += "== Description ==\n\nDoes things.\n\n" }
        if let upgradeNotice { text += "== Upgrade Notice ==\n\n= 1.1.0 =\n\(upgradeNotice)\n\n= 1.0.0 =\nOld notice.\n\n" }
        text += "== Changelog ==\n\n= 1.1.0 =\n* Release.\n" + String(repeating: "* Older entry.\n", count: padding)
        return ReadmeParser.parse(text)
    }

    func messages(_ results: [ValidationResult]) -> [String] { results.map(\.message) }

    func testParsesShortDescriptionAndUpgradeNotice() {
        let parsed = readme(short: "A sample\n  plugin.", upgradeNotice: "Update now.")
        XCTAssertEqual(parsed.shortDescription, "A sample plugin.")
        XCTAssertEqual(parsed.upgradeNotice(for: "1.1.0"), "Update now.")
        XCTAssertEqual(parsed.upgradeNotice(for: "1.0.0"), "Old notice.")
        XCTAssertNil(parsed.upgradeNotice(for: "2.0.0"))
        XCTAssertNotNil(parsed.byteCount)
        XCTAssertEqual(parsed.sections["description"], "Does things.", "sections still parse")
    }

    func testCleanReadmeHasNoWarnings() {
        XCTAssertEqual(ReadmeRules.check(readme(upgradeNotice: "Update now."), file: "readme.txt", version: "1.1.0"), [])
    }

    func testShortDescriptionLimitCountsVisibleText() {
        let link = "[\(String(repeating: "a", count: 140))](#a-very-long-anchor-that-is-not-shown)"
        XCTAssertEqual(ReadmeRules.check(readme(short: link), file: "readme.txt", version: nil), [], "link targets do not count")
        let long = String(repeating: "word ", count: 40)
        let results = ReadmeRules.check(readme(short: long), file: "readme.txt", version: nil)
        XCTAssertEqual(messages(results), ["The short description is 199 characters; WordPress.org cuts it at 150"])
        XCTAssertEqual(results.first?.status, .warning)
    }

    func testShortDescriptionWithColonAfterBlankLineIsKept() {
        let parsed = readme(short: "Gallery for WordPress: fast and simple.")
        XCTAssertEqual(parsed.shortDescription, "Gallery for WordPress: fast and simple.", "matches the WordPress.org parser")
        XCTAssertEqual(ReadmeRules.check(parsed, file: "readme.txt", version: nil), [])
    }

    func testShortDescriptionWithColonDirectlyUnderHeadersIsLost() {
        let parsed = ReadmeParser.parse("=== P ===\nStable tag: 1.0\nGallery for WordPress: fast.\n\n== Description ==\nx\n")
        XCTAssertNil(parsed.shortDescription)
        let results = ReadmeRules.check(parsed, file: "readme.txt", version: nil)
        XCTAssertEqual(results.count, 1)
        XCTAssertTrue(results[0].message.contains("as a header because it contains \":\""), results[0].message)
    }

    func testHeaderKeysAndValuesAreTrimmedLikeWordPressOrg() {
        let parsed = ReadmeParser.parse("=== P ===\n* Tested up to: 7.1\n- Donate link: <https://example.com>\n\nShort.\n")
        XCTAssertEqual(parsed.testedUpTo, "7.1")
        XCTAssertEqual(parsed.fields["donate link"], "https://example.com")
        XCTAssertEqual(parsed.shortDescription, "Short.")
    }

    func testMissingShortDescriptionAndDescriptionSection() {
        let results = ReadmeRules.check(readme(short: nil, description: false), file: "readme.txt", version: nil)
        XCTAssertEqual(messages(results), ["readme has no short description below the headers", "readme has no == Description == section"])
    }

    func testLongUpgradeNoticeWarnsForThemesOnly() {
        let long = readme(upgradeNotice: String(repeating: "x", count: 301))
        XCTAssertEqual(messages(ReadmeRules.check(long, file: "readme.txt", version: "1.1.0", type: .theme)), ["The upgrade notice for 1.1.0 is 301 characters; the theme handbook allows 300"])
        XCTAssertEqual(ReadmeRules.check(long, file: "readme.txt", version: "1.1.0", type: .plugin), [])
    }

    func testLargeReadmeAndLongChangelog() {
        let results = ReadmeRules.check(readme(padding: 1700), file: "readme.txt", version: nil)
        XCTAssertEqual(messages(results).count, 2, "\(messages(results))")
        XCTAssertTrue(results.contains { $0.message == "The Changelog section has 5105 words; WordPress.org cuts it at 5000" && $0.suggestion == "Move older entries to changelog.txt." })
        XCTAssertTrue(results.contains { $0.message.contains("recommends under 10 KB") })
    }

    func testRequiresPlugins() {
        XCTAssertEqual(ReadmeRules.checkRequiresPlugins("woocommerce, advanced-custom-fields", slug: "sample-plugin", file: "p.php"), [])
        let results = ReadmeRules.checkRequiresPlugins("woocommerce/woocommerce.php, Sample_Plugin, sample-plugin", slug: "sample-plugin", file: "p.php")
        XCTAssertEqual(messages(results), [
            "Requires Plugins entry \"woocommerce/woocommerce.php\" is not a WordPress.org slug; WordPress ignores it",
            "Requires Plugins entry \"Sample_Plugin\" is not a WordPress.org slug; WordPress ignores it",
            "Requires Plugins lists the plugin itself",
        ])
        XCTAssertEqual(ReadmeRules.checkRequiresPlugins(nil, slug: "x", file: nil), [])
    }

    func testRulesRunForPluginsDuringValidation() throws {
        let dir = TempDir()
        let main = Fixtures.pluginMainFile(version: "1.1.0").replacingOccurrences(of: " * Requires PHP: 7.4", with: " * Requires PHP: 7.4\n * Requires Plugins: woo/woo.php")
        let root = Fixtures.makePlugin(in: dir, version: "1.1.0", extraFiles: ["sample-plugin.php": main])
        let inspection = try FolderInspectionService().inspect(folderAt: root, options: InspectionOptions())
        let detected = try ProductDetector().detect(at: root, expectedType: .plugin, slug: "sample-plugin")
        let report = ValidationService().validate(ValidationContext(product: makeProduct(), inspection: inspection, detected: detected, repository: RepositorySnapshot(repositoryExists: true, releasePathExists: false, authentication: .keychain)))
        XCTAssertTrue(report.warnings.contains { $0.message.contains("woo/woo.php") && $0.filePath == "sample-plugin.php" })
        XCTAssertFalse(report.warnings.contains { $0.message.contains("short description") }, "the fixture readme has one")
    }
}
