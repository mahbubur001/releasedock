//! Ported from ReadmeRulesTests.swift: the `ReadmeRules` assertions. The parser-only tests and
//! assertions are in rd-versioning (`tests/versioning.rs`).

mod common;

use common::{TempDir, default_plugin_main_file, detect, inspect, make_plugin, make_product};
use rd_model::{ProductType, ValidationResult, ValidationStatus};
use rd_records::{AuthenticationAvailability, RepositorySnapshot};
use rd_validation::readme_rules::{check, check_requires_plugins};
use rd_validation::{ValidationContext, ValidationService, ValidationServicing};
use rd_versioning::{WordPressReadme, readme};

/// Swift `readme(short:description:upgradeNotice:padding:)`.
fn make_readme(
    short: Option<&str>,
    description: bool,
    upgrade_notice: Option<&str>,
    padding: usize,
) -> WordPressReadme {
    let mut text = "=== Sample Plugin ===
Contributors: testauthor
Tags: testing
Tested up to: 7.1
Stable tag: 1.1.0
License: GPLv2 or later
"
    .to_owned();
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
    text += &"* Older entry.\n".repeat(padding);
    readme::parse(&text)
}

fn default_readme() -> WordPressReadme {
    make_readme(Some("A sample plugin."), true, None, 0)
}

fn messages(results: &[ValidationResult]) -> Vec<&str> {
    results.iter().map(|r| r.message.as_str()).collect()
}

#[test]
fn clean_readme_has_no_warnings() {
    let readme = make_readme(Some("A sample plugin."), true, Some("Update now."), 0);
    assert_eq!(
        check(&readme, Some("readme.txt"), Some("1.1.0"), ProductType::Plugin),
        Vec::new()
    );
    assert_eq!(
        check(
            &default_readme(),
            Some("readme.txt"),
            Some("1.1.0"),
            ProductType::Plugin
        ),
        Vec::new()
    );
}

#[test]
fn short_description_limit_counts_visible_text() {
    let link = format!("[{}](#a-very-long-anchor-that-is-not-shown)", "a".repeat(140));
    assert_eq!(
        check(
            &make_readme(Some(&link), true, None, 0),
            Some("readme.txt"),
            None,
            ProductType::Plugin
        ),
        Vec::new(),
        "link targets do not count"
    );
    let long = "word ".repeat(40);
    let results = check(
        &make_readme(Some(&long), true, None, 0),
        Some("readme.txt"),
        None,
        ProductType::Plugin,
    );
    assert_eq!(
        messages(&results),
        ["The short description is 199 characters; WordPress.org cuts it at 150"]
    );
    assert_eq!(results.first().map(|r| r.status), Some(ValidationStatus::Warning));
}

#[test]
fn short_description_with_colon_after_blank_line_is_kept() {
    let parsed = make_readme(Some("Gallery for WordPress: fast and simple."), true, None, 0);
    assert_eq!(
        parsed.short_description.as_deref(),
        Some("Gallery for WordPress: fast and simple."),
        "matches the WordPress.org parser"
    );
    assert_eq!(
        check(&parsed, Some("readme.txt"), None, ProductType::Plugin),
        Vec::new()
    );
}

#[test]
fn short_description_with_colon_directly_under_headers_is_lost() {
    let parsed = readme::parse("=== P ===\nStable tag: 1.0\nGallery for WordPress: fast.\n\n== Description ==\nx\n");
    assert_eq!(parsed.short_description, None);
    let results = check(&parsed, Some("readme.txt"), None, ProductType::Plugin);
    assert_eq!(results.len(), 1);
    assert!(
        results[0].message.contains("as a header because it contains \":\""),
        "{}",
        results[0].message
    );
}

#[test]
fn missing_short_description_and_description_section() {
    let results = check(
        &make_readme(None, false, None, 0),
        Some("readme.txt"),
        None,
        ProductType::Plugin,
    );
    assert_eq!(
        messages(&results),
        [
            "readme has no short description below the headers",
            "readme has no == Description == section"
        ]
    );
}

#[test]
fn long_upgrade_notice_warns_for_themes_only() {
    let notice = "x".repeat(301);
    let long = make_readme(Some("A sample plugin."), true, Some(&notice), 0);
    assert_eq!(
        messages(&check(&long, Some("readme.txt"), Some("1.1.0"), ProductType::Theme)),
        ["The upgrade notice for 1.1.0 is 301 characters; the theme handbook allows 300"]
    );
    assert_eq!(
        check(&long, Some("readme.txt"), Some("1.1.0"), ProductType::Plugin),
        Vec::new()
    );
}

#[test]
fn large_readme_and_long_changelog() {
    let results = check(
        &make_readme(Some("A sample plugin."), true, None, 1700),
        Some("readme.txt"),
        None,
        ProductType::Plugin,
    );
    assert_eq!(messages(&results).len(), 2, "{:?}", messages(&results));
    assert!(results.iter().any(
        |r| r.message == "The Changelog section has 5105 words; WordPress.org cuts it at 5000"
            && r.suggestion.as_deref() == Some("Move older entries to changelog.txt.")
    ));
    assert!(results.iter().any(|r| r.message.contains("recommends under 10 KB")));
}

#[test]
fn requires_plugins() {
    assert_eq!(
        check_requires_plugins(
            Some("woocommerce, advanced-custom-fields"),
            "sample-plugin",
            Some("p.php")
        ),
        Vec::new()
    );
    let results = check_requires_plugins(
        Some("woocommerce/woocommerce.php, Sample_Plugin, sample-plugin"),
        "sample-plugin",
        Some("p.php"),
    );
    assert_eq!(
        messages(&results),
        [
            "Requires Plugins entry \"woocommerce/woocommerce.php\" is not a WordPress.org slug; WordPress ignores it",
            "Requires Plugins entry \"Sample_Plugin\" is not a WordPress.org slug; WordPress ignores it",
            "Requires Plugins lists the plugin itself",
        ]
    );
    assert_eq!(check_requires_plugins(None, "x", None), Vec::new());
}

#[test]
fn rules_run_for_plugins_during_validation() {
    let dir = TempDir::new();
    let main = default_plugin_main_file("1.1.0").replace(
        " * Requires PHP: 7.4",
        " * Requires PHP: 7.4\n * Requires Plugins: woo/woo.php",
    );
    let root = make_plugin(&dir, "1.1.0", None, &[("sample-plugin.php", &main)]);
    let inspection = inspect(&root);
    let detected = detect(&root, Some(ProductType::Plugin), Some("sample-plugin"));
    let repo = RepositorySnapshot {
        repository_exists: Some(true),
        release_path_exists: Some(false),
        authentication: AuthenticationAvailability::Keychain,
        ..RepositorySnapshot::default()
    };
    let report = ValidationService::new().validate(&ValidationContext::new(
        make_product(),
        inspection,
        Some(detected),
        Some(repo),
    ));
    assert!(
        report
            .warnings()
            .iter()
            .any(|r| r.message.contains("woo/woo.php") && r.file_path.as_deref() == Some("sample-plugin.php"))
    );
    assert!(
        !report
            .warnings()
            .iter()
            .any(|r| r.message.contains("short description")),
        "the fixture readme has one"
    );
}

// Added beyond the Swift suite.

#[test]
fn plain_text_strips_links_and_emphasis() {
    use rd_validation::readme_rules::plain_text;
    assert_eq!(
        plain_text("  **Fast** [docs](https://x.y) and `code`_ \n"),
        "Fast docs and code"
    );
}

#[test]
fn short_description_counts_characters_not_bytes() {
    // 150 visible characters, each several bytes or scalars, is within the limit.
    let short = "é".repeat(75) + &"👍🏽".repeat(75);
    assert_eq!(
        check(
            &make_readme(Some(&short), true, None, 0),
            Some("readme.txt"),
            None,
            ProductType::Plugin
        ),
        Vec::new()
    );
}
