//! Ported from WordPressOrgTests.swift: `WordPressOrgTests.testBranches` and
//! `TestedUpToFreshnessTests`. The version-check parsing tests belong to the WordPress.org
//! service crate.

mod common;

use common::{TempDir, default_plugin_main_file, detect, inspect, make_plugin, make_product};
use rd_model::{ProductType, ValidationCheckId, ValidationReport, ValidationResult, ValidationStatus};
use rd_records::{AuthenticationAvailability, RepositorySnapshot};
use rd_validation::{ValidationContext, ValidationService, ValidationServicing, WordPressBranch};

#[test]
fn branches() {
    assert_eq!(WordPressBranch::of("7.1.5").as_deref(), Some("7.1"));
    assert_eq!(WordPressBranch::of("7.1").as_deref(), Some("7.1"));
    assert_eq!(WordPressBranch::of("7").as_deref(), Some("7.0"));
    assert_eq!(WordPressBranch::of("WP 6.9"), None);
    assert_eq!(WordPressBranch::compare("6.9", "7.1.5"), Some(-1));
    assert_eq!(WordPressBranch::compare("7.1", "7.1.5"), Some(0));
    assert_eq!(
        WordPressBranch::compare("7.1.2", "7.1.5"),
        Some(0),
        "point releases share a branch"
    );
    assert_eq!(WordPressBranch::compare("7.2", "7.1.5"), Some(1));
    assert_eq!(WordPressBranch::compare("soon", "7.1"), None);
}

/// Fixture plugins declare "Tested up to: 6.8".
fn report(latest: Option<&str>, main_file_header: Option<&str>) -> ValidationReport {
    let dir = TempDir::new();
    let main;
    let mut extra = Vec::new();
    if let Some(header) = main_file_header {
        main = default_plugin_main_file("1.1.0").replace(
            " * Requires PHP: 7.4",
            &format!(" * Requires PHP: 7.4\n * Tested up to: {header}"),
        );
        extra.push(("sample-plugin.php", main.as_str()));
    }
    let root = make_plugin(&dir, "1.1.0", None, &extra);
    let inspection = inspect(&root);
    let detected = detect(&root, Some(ProductType::Plugin), Some("sample-plugin"));
    let repo = RepositorySnapshot {
        repository_exists: Some(true),
        current_version: Some("1.0.0".into()),
        release_path_exists: Some(false),
        authentication: AuthenticationAvailability::Keychain,
        latest_wordpress_version: latest.map(str::to_owned),
        ..RepositorySnapshot::default()
    };
    ValidationService::new().validate(&ValidationContext::new(
        make_product(),
        inspection,
        Some(detected),
        Some(repo),
    ))
}

fn tested(report: &ValidationReport) -> Vec<&ValidationResult> {
    report
        .results
        .iter()
        .filter(|r| r.check == ValidationCheckId::TestedUpTo)
        .collect()
}

fn statuses(results: &[&ValidationResult]) -> Vec<ValidationStatus> {
    results.iter().map(|r| r.status).collect()
}

#[test]
fn outdated_tested_up_to_warns_with_fix() {
    let report = report(Some("7.1.5"), None);
    let results = tested(&report);
    assert_eq!(results.len(), 1, "the format-only pass is replaced: {results:?}");
    let warning = results[0];
    assert_eq!(warning.status, ValidationStatus::Warning);
    assert!(warning.message.contains("6.8") && warning.message.contains("7.1.5"));
    assert_eq!(warning.file_path.as_deref(), Some("readme.txt"));
    assert_eq!(
        warning.suggestion.as_deref(),
        Some("Test with WordPress 7.1, then set \"Tested up to: 7.1\".")
    );
}

#[test]
fn current_branch_passes() {
    let report = report(Some("6.8.3"), None);
    let results = tested(&report);
    assert_eq!(statuses(&results), [ValidationStatus::Passed]);
    assert!(results[0].message.contains("covers"));
}

#[test]
fn unknown_latest_version_keeps_format_check_only() {
    let report = report(None, None);
    let results = tested(&report);
    assert_eq!(statuses(&results), [ValidationStatus::Passed]);
    assert_eq!(results[0].message, "Tested up to 6.8");
}

#[test]
fn plugin_header_mismatch_warns() {
    let report = report(Some("6.8.1"), Some("6.7"));
    let results = tested(&report);
    assert!(
        results.iter().any(|r| r.status == ValidationStatus::Warning
            && r.message.contains("6.7")
            && r.file_path.as_deref() == Some("sample-plugin.php")),
        "{results:?}"
    );
    assert!(!results.iter().any(|r| r.status == ValidationStatus::Passed));
}

#[test]
fn matching_plugin_header_does_not_warn() {
    let report = report(Some("6.8.1"), Some("6.8"));
    assert_eq!(statuses(&tested(&report)), [ValidationStatus::Passed]);
}

#[test]
fn outdated_tested_up_to_never_blocks() {
    assert!(!report(Some("7.1.5"), None).is_blocking());
}

#[test]
fn disabled_check_is_dropped() {
    let mut product = make_product();
    product.validation.disabled_checks = [ValidationCheckId::TestedUpTo].into();
    let dir = TempDir::new();
    let root = make_plugin(&dir, "1.1.0", None, &[]);
    let inspection = inspect(&root);
    let detected = detect(&root, Some(ProductType::Plugin), Some("sample-plugin"));
    let repo = RepositorySnapshot {
        repository_exists: Some(true),
        release_path_exists: Some(false),
        authentication: AuthenticationAvailability::Keychain,
        latest_wordpress_version: Some("7.1.5".into()),
        ..RepositorySnapshot::default()
    };
    let report =
        ValidationService::new().validate(&ValidationContext::new(product, inspection, Some(detected), Some(repo)));
    assert!(tested(&report).is_empty());
}
