//! Ported from AssetRulesTests.swift: `testAssetWarningsDoNotBlockAndCanBeDisabled`. The asset
//! rule and inspector tests are in rd-inspection (`tests/asset_rules.rs`).

mod common;

use common::{TempDir, detect, inspect, make_plugin, make_product};
use rd_inspection::{AssetFile, AssetsReport};
use rd_model::{ProductType, ValidationCheckId};
use rd_records::{AuthenticationAvailability, RepositorySnapshot};
use rd_validation::{ValidationContext, ValidationService, ValidationServicing};

#[test]
fn asset_warnings_do_not_block_and_can_be_disabled() {
    let assets = AssetsReport::new(vec![AssetFile::new("banner.png", 1)]);
    let dir = TempDir::new();
    let root = make_plugin(&dir, "1.1.0", None, &[]);
    let inspection = inspect(&root);
    let detected = detect(&root, Some(ProductType::Plugin), Some("sample-plugin"));
    let repo = RepositorySnapshot {
        repository_exists: Some(true),
        current_version: Some("1.0.0".into()),
        release_path_exists: Some(false),
        authentication: AuthenticationAvailability::Keychain,
        ..RepositorySnapshot::default()
    };
    let mut product = make_product();
    let mut context = ValidationContext::new(product.clone(), inspection, Some(detected), Some(repo));
    context.assets = Some(assets);
    let report = ValidationService::new().validate(&context);
    assert!(!report.is_blocking());
    assert!(
        report
            .warnings()
            .iter()
            .any(|r| r.check == ValidationCheckId::DirectoryAssets)
    );
    product.validation.disabled_checks = [ValidationCheckId::DirectoryAssets].into();
    context.product = product;
    let report = ValidationService::new().validate(&context);
    assert!(
        !report
            .results
            .iter()
            .any(|r| r.check == ValidationCheckId::DirectoryAssets)
    );
}
