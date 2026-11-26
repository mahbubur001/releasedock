//! Ported from VersionTests.swift (PHPVersionCompareTests, VersionServiceTests,
//! HeaderAndReadmeTests), the parser tests of ReadmeRulesTests.swift and ChangelogEntryTests.
#![allow(clippy::unwrap_used)] // Test helpers outside #[test] functions.

mod common;

use common::TempDir;
use rd_versioning::version_service::{VersionComparison, VersionService, VersionServicing, VersionValidity};
use rd_versioning::{headers, php_version, readme};
use std::io::Write;
use std::process::{Command, Stdio};

// MARK: PHPVersionCompareTests

#[test]
fn known_orderings() {
    let cases = [
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
    ];
    for (a, b, expected) in cases {
        assert_eq!(php_version::compare(a, b), expected, "version_compare('{a}', '{b}')");
        assert_eq!(php_version::compare(b, a), -expected, "version_compare('{b}', '{a}')");
    }
}

#[test]
fn canonicalization() {
    assert_eq!(php_version::canonicalize("1.0rc1"), "1.0.rc.1");
    assert_eq!(php_version::canonicalize("1.0-beta_2+build"), "1.0.beta.2.build");
    assert_eq!(php_version::canonicalize("1..2"), "1.2");
}

/// The 1,024 pairs of `testMatchesInstalledPHP`, with results recorded from PHP 8.5.7.
const PHP_FIXTURE: &str = include_str!("fixtures/php_version_compare.tsv");

fn fixture_pairs() -> Vec<(String, String, i32)> {
    PHP_FIXTURE
        .lines()
        .filter(|line| !line.starts_with("# "))
        .map(|line| {
            let fields: Vec<&str> = line.split('\t').collect();
            assert_eq!(fields.len(), 3, "{line:?}");
            (fields[0].to_string(), fields[1].to_string(), fields[2].parse().unwrap())
        })
        .collect()
}

#[test]
fn matches_recorded_php_results() {
    let pairs = fixture_pairs();
    assert_eq!(pairs.len(), 1024);
    for (a, b, expected) in pairs {
        assert_eq!(php_version::compare(&a, &b), expected, "version_compare('{a}', '{b}')");
    }
}

/// Differential test against the real PHP interpreter when it is installed.
#[test]
fn matches_installed_php() {
    let php = ["/opt/homebrew/bin/php", "/usr/local/bin/php", "/usr/bin/php"]
        .into_iter()
        .find(|p| std::path::Path::new(p).is_file());
    let Some(php) = php else {
        eprintln!("skipped: php is not installed");
        return;
    };
    let pairs = fixture_pairs();
    let input: Vec<[&str; 2]> = pairs.iter().map(|(a, b, _)| [a.as_str(), b.as_str()]).collect();
    let script = r#"foreach (json_decode(file_get_contents('php://stdin'), true) as $p) { echo version_compare($p[0], $p[1]), "\n"; }"#;
    let mut child = Command::new(php)
        .args(["-r", script])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(serde_json::to_string(&input).unwrap().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    let results: Vec<i32> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|l| l.parse().ok())
        .collect();
    assert_eq!(results.len(), pairs.len());
    for ((a, b, _), expected) in pairs.iter().zip(results) {
        assert_eq!(php_version::compare(a, b), expected, "version_compare('{a}', '{b}')");
    }
}

// MARK: VersionServiceTests

#[test]
fn validity() {
    let service = VersionService;
    assert_eq!(
        service.validate("1.2.3"),
        VersionValidity::Valid { is_prerelease: false }
    );
    assert_eq!(
        service.validate("1.2.3-beta1"),
        VersionValidity::Valid { is_prerelease: true }
    );
    assert_eq!(
        service.validate("2.0.0-RC.1"),
        VersionValidity::Valid { is_prerelease: true }
    );
    assert_eq!(
        service.validate("1.0pl1"),
        VersionValidity::Valid { is_prerelease: false }
    );
    assert!(!service.validate("").is_valid());
    assert!(!service.validate("v1.2.3").is_valid());
    assert!(!service.validate(" 1.2").is_valid());
    assert!(!service.validate("1.2.").is_valid());
    assert!(!service.validate("1..2").is_valid());
    assert!(!service.validate("1.2 beta").is_valid());
    assert!(
        !service.validate("1.0-hotfix").is_valid(),
        "unknown words sort below dev in PHP"
    );
    assert!(
        !service.validate("1.0-pre").is_valid(),
        "pre sorts as a patch level in PHP"
    );
    assert!(
        !service.validate("1.0-Beta").is_valid(),
        "capitalised Beta is unknown to PHP"
    );
}

#[test]
fn validity_messages() {
    let reason = |v: &str| match VersionService.validate(v) {
        VersionValidity::Invalid { reason } => reason,
        valid => panic!("{v:?} is {valid:?}"),
    };
    assert_eq!(reason(""), "Version is empty");
    assert_eq!(reason(" 1.2"), "Version has surrounding whitespace");
    assert_eq!(reason(&"1".repeat(65)), "Version is longer than 64 characters");
    assert_eq!(reason("v1.2.3"), "Version must start with a digit (found \"v1.2.3\")");
    assert_eq!(
        reason("1.2 beta"),
        "Version contains characters other than letters, digits, '.', '-', '+' or '_'"
    );
    assert_eq!(reason("1..2"), "Version has an empty component");
    assert_eq!(
        reason("1.0-hotfix"),
        "Version identifier \"hotfix\" has no reliable order in PHP version_compare()"
    );
}

#[test]
fn comparison_rules() {
    let service = VersionService;
    assert_eq!(service.compare("1.2.4", Some("1.2.3")), VersionComparison::Newer);
    assert_eq!(service.compare("1.2.3", Some("1.2.3")), VersionComparison::Same);
    assert_eq!(service.compare("1.2.2", Some("1.2.3")), VersionComparison::Older);
    assert_eq!(service.compare("1.0rc1", Some("1.0-RC-1")), VersionComparison::Same);
    assert_eq!(service.compare("1.0.0", None), VersionComparison::NoCurrentRelease);
    assert_eq!(
        service.compare("1.0.0", Some("trunk")),
        VersionComparison::NoCurrentRelease
    );
    assert!(
        matches!(
            service.compare("v2", Some("1.0")),
            VersionComparison::RequiresReview { .. }
        ),
        "invalid candidate must require review"
    );
    assert!(
        matches!(
            service.compare("2.0", Some("latest")),
            VersionComparison::RequiresReview { .. }
        ),
        "invalid current must require review"
    );
    assert!(VersionComparison::Newer.allows_release());
    assert!(!VersionComparison::Same.allows_release());
    assert!(!VersionComparison::Older.allows_release());
}

#[test]
fn highest() {
    let service = VersionService;
    let versions: Vec<String> = ["1.0", "1.10", "1.9", "junk", "1.2-beta"].map(String::from).to_vec();
    assert_eq!(service.highest(&versions).as_deref(), Some("1.10"));
    assert_eq!(service.highest(&[]), None);
}

// MARK: HeaderAndReadmeTests

#[test]
fn plugin_headers_follow_get_file_data() {
    let php = "<?php
/*
 * Plugin Name:  My Plugin */
 * Version: 2.1.0 ?> trailing
 * text domain: my-plugin
 * Requires PHP:
 */";
    let headers = headers::parse(php, headers::PLUGIN_HEADERS);
    assert_eq!(headers.get("Plugin Name").map(String::as_str), Some("My Plugin"));
    assert_eq!(headers.get("Version").map(String::as_str), Some("2.1.0"));
    assert_eq!(
        headers.get("Text Domain").map(String::as_str),
        Some("my-plugin"),
        "matching is case-insensitive"
    );
    assert_eq!(headers.get("Requires PHP"), None, "empty values count as missing");
}

#[test]
fn header_on_first_line_and_cr_only_line_endings() {
    let headers = headers::parse(
        "<?php // Plugin Name: Inline\r * Version: 1.0\r",
        &["Plugin Name", "Version"],
    );
    assert_eq!(headers.get("Version").map(String::as_str), Some("1.0"));
    assert_eq!(
        headers.get("Plugin Name").map(String::as_str),
        Some("Inline"),
        "slashes are allowed before the header name"
    );
    let first = headers::parse("<?php Plugin Name: Inline", &["Plugin Name"]);
    assert_eq!(first.get("Plugin Name").map(String::as_str), Some("Inline"));
}

#[test]
fn only_first_8kb_is_read() {
    let dir = TempDir::new();
    let padding = "x".repeat(9000);
    let file = dir.write("p.php", &format!("<?php\n/* {padding} */\n/* Version: 9.9 */\n"));
    assert_eq!(headers::read(&file, &["Version"]).unwrap().get("Version"), None);
}

#[test]
fn readme_parsing() {
    let readme = readme::parse(
        "\u{FEFF}=== My Plugin ===
Contributors: alice, @bob
Tags: a, b, c
Requires at least: 6.0
Tested: 6.8
Stable tag: \"tags/1.2.3\"
License: GPLv2

Short description.

== Changelog ==

= 1.2.3 - 2026-11-01 =
* Fix.

= 1.2 =
* Old.",
    );
    assert_eq!(readme.name.as_deref(), Some("My Plugin"));
    assert_eq!(
        readme.fields.get("tested up to").map(String::as_str),
        Some("6.8"),
        "Tested alias"
    );
    assert_eq!(readme.raw_stable_tag(), Some("\"tags/1.2.3\""));
    assert_eq!(
        readme.stable_tag().as_deref(),
        Some("1.2.3"),
        "sanitized like WordPress.org"
    );
    assert_eq!(readme.contributors(), ["alice", "@bob"]);
    assert!(readme.changelog_mentions("1.2.3"));
    assert!(readme.changelog_mentions("1.2"));
    assert!(!readme.changelog_mentions("1.2.4"));
    assert!(!readme::parse("== Changelog ==\n= 1.2.3 =\n").changelog_mentions("1.2"));
}

#[test]
fn stable_tag_sanitization() {
    assert_eq!(readme::sanitize_stable_tag(" '/tags/2.0' "), "2.0");
    assert_eq!(readme::sanitize_stable_tag(".5"), "0.5");
    assert_eq!(readme::sanitize_stable_tag("1.0 (beta)"), "1.0beta");
}

// MARK: ReadmeRulesTests (parser assertions only; the rules are ported with validation)

fn rules_readme(
    short: Option<&str>,
    description: bool,
    upgrade_notice: Option<&str>,
) -> rd_versioning::WordPressReadme {
    let mut text = String::from(
        "=== Sample Plugin ===
Contributors: testauthor
Tags: testing
Tested up to: 7.1
Stable tag: 1.1.0
License: GPLv2 or later
",
    );
    if let Some(short) = short {
        text += &format!("\n{short}\n\n");
    }
    if description {
        text += "== Description ==\n\nDoes things.\n\n";
    }
    if let Some(notice) = upgrade_notice {
        text += &format!("== Upgrade Notice ==\n\n= 1.1.0 =\n{notice}\n\n= 1.0.0 =\nOld notice.\n\n");
    }
    text += "== Changelog ==\n\n= 1.1.0 =\n* Release.\n";
    readme::parse(&text)
}

#[test]
fn parses_short_description_and_upgrade_notice() {
    let parsed = rules_readme(Some("A sample\n  plugin."), true, Some("Update now."));
    assert_eq!(parsed.short_description.as_deref(), Some("A sample plugin."));
    assert_eq!(parsed.upgrade_notice("1.1.0").as_deref(), Some("Update now."));
    assert_eq!(parsed.upgrade_notice("1.0.0").as_deref(), Some("Old notice."));
    assert_eq!(parsed.upgrade_notice("2.0.0"), None);
    assert!(parsed.byte_count.is_some());
    assert_eq!(
        parsed.sections.get("description").map(String::as_str),
        Some("Does things."),
        "sections still parse"
    );
}

#[test]
fn short_description_with_colon_after_blank_line_is_kept() {
    let parsed = rules_readme(Some("Gallery for WordPress: fast and simple."), true, None);
    assert_eq!(
        parsed.short_description.as_deref(),
        Some("Gallery for WordPress: fast and simple."),
        "matches the WordPress.org parser"
    );
    assert_eq!(parsed.unknown_header_lines, None);
}

#[test]
fn short_description_with_colon_directly_under_headers_is_lost() {
    let parsed = readme::parse("=== P ===\nStable tag: 1.0\nGallery for WordPress: fast.\n\n== Description ==\nx\n");
    assert_eq!(parsed.short_description, None);
    assert_eq!(
        parsed.unknown_header_lines,
        Some(vec!["Gallery for WordPress: fast.".to_string()])
    );
}

#[test]
fn header_keys_and_values_are_trimmed_like_wordpress_org() {
    let parsed = readme::parse("=== P ===\n* Tested up to: 7.1\n- Donate link: <https://example.com>\n\nShort.\n");
    assert_eq!(parsed.tested_up_to(), Some("7.1"));
    assert_eq!(
        parsed.fields.get("donate link").map(String::as_str),
        Some("https://example.com")
    );
    assert_eq!(parsed.short_description.as_deref(), Some("Short."));
}

// MARK: ChangelogEntryTests

#[test]
fn extracts_entries_in_common_heading_styles() {
    let readme = readme::parse(
        "=== P ===
Stable tag: 1.2.0

Short.

== Changelog ==

= 1.2.0 - 2026-11-24 =
* New: Lightbox.
* Fix: RTL.

= 1.1.0 =
* Older.

### v1.0.0 (2026-01-01)
* First.",
    );
    assert_eq!(
        readme.changelog_entry("1.2.0").as_deref(),
        Some("* New: Lightbox.\n* Fix: RTL.")
    );
    assert_eq!(readme.changelog_entry("1.1.0").as_deref(), Some("* Older."));
    assert_eq!(readme.changelog_entry("1.0.0").as_deref(), Some("* First."));
    assert_eq!(readme.changelog_entry("1.2"), None, "no prefix matches");
    assert_eq!(readme.changelog_entry("9.9.9"), None);
}
