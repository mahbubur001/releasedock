//! The Rust types must read and write the Swift app's stored JSON unchanged.

use rd_model::*;

const SWIFT_PRODUCT: &str = r#"{
  "automationEnabled": false,
  "createdAt": 817029777.884127,
  "currentVersion": "1.0.24",
  "expectedHeaderName": "Sample Plugin",
  "id": "EE90AEAA-DAB2-45F4-805D-58DE936B984C",
  "lastSuccessfulRelease": {"date": 817293629.069138, "releaseID": "34629C18-132D-4671-9B07-748F19721D10", "revision": 3822482, "version": "1.0.24"},
  "name": "Sample Plugin",
  "releaseMode": "assisted",
  "slug": "sample-plugin",
  "svnRepositoryURL": "https://plugins.svn.wordpress.org/sample-plugin",
  "type": "plugin",
  "updatedAt": 817293629.069138,
  "validation": {"blockOnWarnings": false, "disabledChecks": ["phpSyntax", "someFutureCheck"], "excludePatterns": [], "packageSizeWarningMB": 10, "requireChangelogEntry": true, "requireCleanGitTree": false},
  "workingCopyPath": "/tmp/svn/sample-plugin"
}"#;

#[test]
fn reads_a_product_stored_by_the_swift_app() {
    let product: Product = serde_json::from_str(SWIFT_PRODUCT).unwrap();
    assert_eq!(product.id.to_string(), "EE90AEAA-DAB2-45F4-805D-58DE936B984C");
    assert_eq!(product.product_type, ProductType::Plugin);
    assert_eq!(
        product.last_successful_release.as_ref().unwrap().revision,
        Some(3822482)
    );
    assert_eq!(
        product.validation.disabled_checks.len(),
        1,
        "unknown check ids are dropped"
    );
    assert!(
        product
            .validation
            .disabled_checks
            .contains(&ValidationCheckId::PhpSyntax)
    );
    // 817029777.884127 s after 2001-01-01 is 2026-11-21 (Unix 1795337 xxx).
    assert!((product.created_at.unix_seconds() - 1_795_336_977.884_127).abs() < 1e-3);
}

#[test]
fn writes_json_the_swift_app_can_read() {
    let product: Product = serde_json::from_str(SWIFT_PRODUCT).unwrap();
    let value = serde_json::to_value(&product).unwrap();
    assert_eq!(value["id"], "EE90AEAA-DAB2-45F4-805D-58DE936B984C", "uppercase UUID");
    assert_eq!(value["type"], "plugin");
    assert_eq!(
        value["svnRepositoryURL"],
        "https://plugins.svn.wordpress.org/sample-plugin"
    );
    assert_eq!(value["validation"]["packageSizeWarningMB"], 10);
    assert!(value.get("assetsPath").is_none(), "absent optionals are omitted");
    assert_eq!(value["createdAt"].as_f64(), Some(817029777.884127));
}

#[test]
fn validation_settings_fill_missing_keys_with_defaults() {
    let settings: ValidationSettings = serde_json::from_str(r#"{"blockOnWarnings": true}"#).unwrap();
    assert!(settings.block_on_warnings);
    assert!(settings.require_changelog_entry);
    assert_eq!(settings.package_size_warning_mb, 10);
}

#[test]
fn blocking_results_follow_status_and_required_checks() {
    let report = ValidationReport {
        results: vec![
            ValidationResult::warn(ValidationCheckId::TestedUpTo, "old"),
            ValidationResult::skip(ValidationCheckId::ReleaseDiff, "later"),
            ValidationResult::skip(ValidationCheckId::SvnConnectivity, "offline"),
            ValidationResult::skip(ValidationCheckId::ChangelogEntry, "off"),
        ],
        block_on_warnings: false,
    };
    let blocking: Vec<_> = report.blocking_results().iter().map(|r| r.check).collect();
    assert_eq!(blocking, [ValidationCheckId::SvnConnectivity]);
    assert_eq!(ValidationCheckId::ReleaseDiff.raw_value(), "releaseDiff");
    assert!(ValidationSeverity::Info < ValidationSeverity::Error);
}
