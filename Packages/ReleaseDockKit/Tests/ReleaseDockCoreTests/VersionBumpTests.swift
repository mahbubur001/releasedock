import XCTest
@testable import ReleaseDockCore

final class VersionBumpTests: XCTestCase {
    let service = VersionBumpService()

    func plugin(_ dir: TempDir, crlf: Bool = false) -> URL {
        let main = Fixtures.pluginMainFile(version: "2.3.0") + """

        define( 'SAMPLE_PLUGIN_VERSION', '2.3.0' );
        define( 'SAMPLE_MIN_WP', '2.3.0' ); // not a version constant name
        """
        let root = Fixtures.makePlugin(in: dir, version: "2.3.0", extraFiles: [
            "sample-plugin.php": crlf ? main.replacingOccurrences(of: "\n", with: "\r\n") : main,
            "includes/class-plugin.php": "<?php\nfinal class Plugin {\n    public const VERSION = '2.3.0';\n    const DB_VERSION = '1.0';\n}\n",
            "vendor/lib/x.php": "<?php define( 'LIB_VERSION', '2.3.0' );\n",
            "package.json": "{\n  \"name\": \"sample-plugin\",\n  \"version\": \"2.3.0\",\n  \"devDependencies\": { \"x\": { \"version\": \"2.3.0\" } }\n}\n",
        ])
        return root
    }

    func testSuggestions() {
        XCTAssertEqual(VersionBumpService.suggest(.patch, from: "2.3.9"), "2.3.10")
        XCTAssertEqual(VersionBumpService.suggest(.minor, from: "2.3.9"), "2.4.0")
        XCTAssertEqual(VersionBumpService.suggest(.major, from: "2.3"), "3.0.0")
        XCTAssertEqual(VersionBumpService.suggest(.patch, from: "1.0.0-beta1"), "1.0.1")
        XCTAssertNil(VersionBumpService.suggest(.patch, from: "1.x"))
    }

    func testPlansEveryReferenceAndAppliesThem() throws {
        let dir = TempDir()
        let root = plugin(dir)
        let date = DateComponents(calendar: .current, year: 2026, month: 11, day: 24, hour: 23).date!
        let plan = try service.plan(folder: root, product: makeProduct(), newVersion: "2.4.0", changelogNotes: "New: Lightbox\n* Fix: RTL\n", today: date)
        XCTAssertEqual(plan.currentVersion, "2.3.0")
        XCTAssertEqual(Set(plan.changes.map(\.description)), ["Version header", "Stable tag", "Changelog entry", "Version constant", "package.json version"])
        XCTAssertEqual(plan.files.sorted(), ["includes/class-plugin.php", "package.json", "readme.txt", "sample-plugin.php"], "vendor is skipped")
        XCTAssertEqual(plan.changes.filter { $0.description == "Version constant" }.count, 2)
        XCTAssertTrue(plan.notes.isEmpty, "\(plan.notes)")
        XCTAssertTrue(FileManager.default.fileExists(atPath: root.path), "planning writes nothing")
        XCTAssertTrue(try String(contentsOf: root.appendingPathComponent("sample-plugin.php"), encoding: .utf8).contains("Version: 2.3.0"))

        try service.apply(plan, folder: root)
        let main = try String(contentsOf: root.appendingPathComponent("sample-plugin.php"), encoding: .utf8)
        XCTAssertTrue(main.contains(" * Version: 2.4.0"))
        XCTAssertTrue(main.contains("define( 'SAMPLE_PLUGIN_VERSION', '2.4.0' );"))
        XCTAssertTrue(main.contains("define( 'SAMPLE_MIN_WP', '2.3.0' );"), "only *VERSION constants")
        let cls = try String(contentsOf: root.appendingPathComponent("includes/class-plugin.php"), encoding: .utf8)
        XCTAssertTrue(cls.contains("public const VERSION = '2.4.0';"))
        XCTAssertTrue(cls.contains("const DB_VERSION = '1.0';"), "other versions untouched")
        let json = try String(contentsOf: root.appendingPathComponent("package.json"), encoding: .utf8)
        XCTAssertTrue(json.contains("\"version\": \"2.4.0\",\n  \"devDependencies\": { \"x\": { \"version\": \"2.3.0\" } }"))
        XCTAssertTrue(try String(contentsOf: root.appendingPathComponent("vendor/lib/x.php"), encoding: .utf8).contains("'2.3.0'"))

        // The result is a valid release: the detector and readme agree on 2.4.0.
        let detected = try ProductDetector().detect(at: root)
        XCTAssertEqual(detected.version, "2.4.0")
        XCTAssertEqual(detected.readme?.stableTag, "2.4.0")
        XCTAssertEqual(detected.readme?.changelogEntry(for: "2.4.0"), "* New: Lightbox\n* Fix: RTL")
        XCTAssertTrue(try String(contentsOf: root.appendingPathComponent("readme.txt"), encoding: .utf8).contains("= 2.4.0 - 2026-11-24 ="), "local date")
    }

    func testMissingChangelogNotesAreReportedNotInvented() throws {
        let dir = TempDir()
        let plan = try service.plan(folder: plugin(dir), product: makeProduct(), newVersion: "2.4.0")
        XCTAssertFalse(plan.changes.contains { $0.description == "Changelog entry" })
        XCTAssertTrue(plan.notes.contains { $0.contains("no changelog entry for 2.4.0") })
    }

    func testRejectsOlderOrInvalidVersions() throws {
        let dir = TempDir()
        let root = plugin(dir)
        XCTAssertThrowsError(try service.plan(folder: root, product: makeProduct(), newVersion: "2.3.0")) {
            XCTAssertEqual($0 as? VersionBumpError, .notNewer(current: "2.3.0", new: "2.3.0"))
        }
        XCTAssertThrowsError(try service.plan(folder: root, product: makeProduct(), newVersion: "two")) {
            XCTAssertEqual($0 as? VersionBumpError, .invalidVersion("two"))
        }
    }

    func testRefusesToOverwriteFilesChangedAfterThePreview() throws {
        let dir = TempDir()
        let root = plugin(dir)
        let plan = try service.plan(folder: root, product: makeProduct(), newVersion: "2.4.0")
        dir.write("sample-plugin/readme.txt", "edited meanwhile")
        XCTAssertThrowsError(try service.apply(plan, folder: root)) {
            XCTAssertEqual($0 as? VersionBumpError, .fileChanged("readme.txt"))
        }
        XCTAssertTrue(try String(contentsOf: root.appendingPathComponent("sample-plugin.php"), encoding: .utf8).contains("Version: 2.3.0"), "nothing written")
    }

    func testKeepsWindowsLineEndings() throws {
        let dir = TempDir()
        let root = plugin(dir, crlf: true)
        try service.apply(try service.plan(folder: root, product: makeProduct(), newVersion: "2.3.1"), folder: root)
        let main = try String(contentsOf: root.appendingPathComponent("sample-plugin.php"), encoding: .utf8)
        XCTAssertTrue(main.contains(" * Version: 2.3.1\r\n"))
        XCTAssertFalse(main.replacingOccurrences(of: "\r\n", with: "").contains("\n"), "no bare LF introduced")
    }

    func testThemeBumpsStyleCSS() throws {
        let dir = TempDir()
        let root = Fixtures.makeTheme(in: dir, version: "1.2.0")
        var theme = makeProduct()
        theme.type = .theme
        let plan = try service.plan(folder: root, product: theme, newVersion: "1.3.0", changelogNotes: "Block patterns")
        XCTAssertEqual(plan.files.sorted(), ["readme.txt", "style.css"])
        XCTAssertFalse(plan.changes.contains { $0.description == "Stable tag" }, "theme readmes have no Stable tag")
        try service.apply(plan, folder: root)
        XCTAssertEqual(try ProductDetector().detect(at: root).version, "1.3.0")
    }
}
