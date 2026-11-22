import XCTest
@testable import ReleaseDockCore

final class PHPVersionCompareTests: XCTestCase {
    func testKnownOrderings() {
        let cases: [(String, String, Int)] = [
            ("1.0", "1.0", 0),
            ("1.0", "1.1", -1),
            ("1.10", "1.9", 1),
            ("1.0", "1.0.0", -1),
            ("1.0.0", "1.0", 1),
            ("1.0-dev", "1.0-alpha", -1),
            ("1.0alpha", "1.0beta", -1),
            ("1.0beta", "1.0RC1", -1),
            ("1.0RC1", "1.0", -1),
            ("1.0", "1.0pl1", -1),
            ("1.0-beta.2", "1.0-beta.10", -1),
            ("1.0rc1", "1.0-RC-1", 0),
            ("2.0.0", "10.0.0", -1),
            ("1.0-hotfix", "1.0", -1), // unknown word sorts below a number
            ("1.0-pre", "1.0", 1),     // "pre" prefix-matches "p" (patch level)
            ("", "1.0", -1),
            ("1.0", "", 1),
            ("v1.2", "1.0", -1),
        ]
        for (a, b, expected) in cases {
            XCTAssertEqual(PHPVersion.compare(a, b), expected, "version_compare('\(a)', '\(b)')")
            XCTAssertEqual(PHPVersion.compare(b, a), -expected, "version_compare('\(b)', '\(a)')")
        }
    }

    func testCanonicalization() {
        XCTAssertEqual(PHPVersion.canonicalize("1.0rc1"), "1.0.rc.1")
        XCTAssertEqual(PHPVersion.canonicalize("1.0-beta_2+build"), "1.0.beta.2.build")
        XCTAssertEqual(PHPVersion.canonicalize("1..2"), "1.2")
    }

    /// Differential test against the real PHP interpreter when it is installed.
    func testMatchesInstalledPHP() throws {
        let php = ["/opt/homebrew/bin/php", "/usr/local/bin/php", "/usr/bin/php"].first { FileManager.default.isExecutableFile(atPath: $0) }
        guard let php else { throw XCTSkip("php is not installed") }
        let versions = ["1", "1.0", "1.0.0", "1.0.1", "1.1", "1.10", "1.2", "2.0", "1.0-dev", "1.0alpha", "1.0a1", "1.0-beta", "1.0b2",
                        "1.0RC1", "1.0rc2", "1.0pl1", "1.0p2", "1.0-patch", "1.0-hotfix", "1.0-pre", "0.9.9", "10.0", "1.0.0.1",
                        "1.0+build.5", "1.0_1", "01.2", "1.02", "3.4.5-beta.1", "3.4.5-beta.10", "1.0-Beta", "#", "1.0#"]
        var pairs: [(String, String)] = []
        for a in versions { for b in versions { pairs.append((a, b)) } }
        let script = "foreach (json_decode(file_get_contents('php://stdin'), true) as $p) { echo version_compare($p[0], $p[1]), \"\\n\"; }"
        let input = try JSONSerialization.data(withJSONObject: pairs.map { [$0.0, $0.1] })
        let process = Process()
        process.executableURL = URL(fileURLWithPath: php)
        process.arguments = ["-r", script]
        let stdin = Pipe(), stdout = Pipe()
        process.standardInput = stdin
        process.standardOutput = stdout
        let done = DispatchSemaphore(value: 0)
        process.terminationHandler = { _ in done.signal() }
        try process.run()
        stdin.fileHandleForWriting.write(input)
        try stdin.fileHandleForWriting.close()
        let output = String(decoding: stdout.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
        done.wait()
        let results = output.split(separator: "\n").compactMap { Int($0) }
        XCTAssertEqual(results.count, pairs.count)
        for (pair, expected) in zip(pairs, results) {
            XCTAssertEqual(PHPVersion.compare(pair.0, pair.1), expected, "version_compare('\(pair.0)', '\(pair.1)')")
        }
    }
}

final class VersionServiceTests: XCTestCase {
    let service = VersionService()

    func testValidity() {
        XCTAssertEqual(service.validate("1.2.3"), .valid(isPrerelease: false))
        XCTAssertEqual(service.validate("1.2.3-beta1"), .valid(isPrerelease: true))
        XCTAssertEqual(service.validate("2.0.0-RC.1"), .valid(isPrerelease: true))
        XCTAssertEqual(service.validate("1.0pl1"), .valid(isPrerelease: false))
        XCTAssertFalse(service.validate("").isValid)
        XCTAssertFalse(service.validate("v1.2.3").isValid)
        XCTAssertFalse(service.validate(" 1.2").isValid)
        XCTAssertFalse(service.validate("1.2.").isValid)
        XCTAssertFalse(service.validate("1..2").isValid)
        XCTAssertFalse(service.validate("1.2 beta").isValid)
        XCTAssertFalse(service.validate("1.0-hotfix").isValid, "unknown words sort below dev in PHP")
        XCTAssertFalse(service.validate("1.0-pre").isValid, "pre sorts as a patch level in PHP")
        XCTAssertFalse(service.validate("1.0-Beta").isValid, "capitalised Beta is unknown to PHP")
    }

    func testComparisonRules() {
        XCTAssertEqual(service.compare(candidate: "1.2.4", current: "1.2.3"), .newer)
        XCTAssertEqual(service.compare(candidate: "1.2.3", current: "1.2.3"), .same)
        XCTAssertEqual(service.compare(candidate: "1.2.2", current: "1.2.3"), .older)
        XCTAssertEqual(service.compare(candidate: "1.0rc1", current: "1.0-RC-1"), .same)
        XCTAssertEqual(service.compare(candidate: "1.0.0", current: nil), .noCurrentRelease)
        XCTAssertEqual(service.compare(candidate: "1.0.0", current: "trunk"), .noCurrentRelease)
        if case .requiresReview = service.compare(candidate: "v2", current: "1.0") {} else { XCTFail("invalid candidate must require review") }
        if case .requiresReview = service.compare(candidate: "2.0", current: "latest") {} else { XCTFail("invalid current must require review") }
        XCTAssertTrue(VersionComparison.newer.allowsRelease)
        XCTAssertFalse(VersionComparison.same.allowsRelease)
        XCTAssertFalse(VersionComparison.older.allowsRelease)
    }

    func testHighest() {
        XCTAssertEqual(service.highest(of: ["1.0", "1.10", "1.9", "junk", "1.2-beta"]), "1.10")
        XCTAssertNil(service.highest(of: []))
    }
}

final class HeaderAndReadmeTests: XCTestCase {
    func testPluginHeadersFollowGetFileData() {
        let php = """
        <?php
        /*
         * Plugin Name:  My Plugin */
         * Version: 2.1.0 ?> trailing
         * text domain: my-plugin
         * Requires PHP:
         */
        """
        let headers = WordPressHeaders.parse(php, headers: WordPressHeaders.pluginHeaders)
        XCTAssertEqual(headers["Plugin Name"], "My Plugin")
        XCTAssertEqual(headers["Version"], "2.1.0")
        XCTAssertEqual(headers["Text Domain"], "my-plugin", "matching is case-insensitive")
        XCTAssertNil(headers["Requires PHP"], "empty values count as missing")
    }

    func testHeaderOnFirstLineAndCROnlyLineEndings() {
        let headers = WordPressHeaders.parse("<?php // Plugin Name: Inline\r * Version: 1.0\r", headers: ["Plugin Name", "Version"])
        XCTAssertEqual(headers["Version"], "1.0")
        XCTAssertEqual(headers["Plugin Name"], "Inline", "slashes are allowed before the header name")
        let first = WordPressHeaders.parse("<?php Plugin Name: Inline", headers: ["Plugin Name"])
        XCTAssertEqual(first["Plugin Name"], "Inline")
    }

    func testOnlyFirst8KBIsRead() throws {
        let dir = TempDir()
        let padding = String(repeating: "x", count: 9000)
        let file = dir.write("p.php", "<?php\n/* \(padding) */\n/* Version: 9.9 */\n")
        XCTAssertNil(try WordPressHeaders.read(file, headers: ["Version"])["Version"])
    }

    func testReadmeParsing() {
        let readme = ReadmeParser.parse("""
        \u{FEFF}=== My Plugin ===
        Contributors: alice, @bob
        Tags: a, b, c
        Requires at least: 6.0
        Tested: 6.8
        Stable tag: "tags/1.2.3"
        License: GPLv2

        Short description.

        == Changelog ==

        = 1.2.3 - 2026-11-01 =
        * Fix.

        = 1.2 =
        * Old.
        """)
        XCTAssertEqual(readme.name, "My Plugin")
        XCTAssertEqual(readme.fields["tested up to"], "6.8", "Tested alias")
        XCTAssertEqual(readme.rawStableTag, "\"tags/1.2.3\"")
        XCTAssertEqual(readme.stableTag, "1.2.3", "sanitized like WordPress.org")
        XCTAssertEqual(readme.contributors, ["alice", "@bob"])
        XCTAssertTrue(readme.changelogMentions(version: "1.2.3"))
        XCTAssertTrue(readme.changelogMentions(version: "1.2"))
        XCTAssertFalse(readme.changelogMentions(version: "1.2.4"))
        XCTAssertFalse(ReadmeParser.parse("== Changelog ==\n= 1.2.3 =\n").changelogMentions(version: "1.2"))
    }

    func testStableTagSanitization() {
        XCTAssertEqual(ReadmeParser.sanitizeStableTag(" '/tags/2.0' "), "2.0")
        XCTAssertEqual(ReadmeParser.sanitizeStableTag(".5"), "0.5")
        XCTAssertEqual(ReadmeParser.sanitizeStableTag("1.0 (beta)"), "1.0beta")
    }
}
