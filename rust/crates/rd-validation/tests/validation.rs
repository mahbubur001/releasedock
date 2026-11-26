//! Ported from ValidationTests.swift (`ValidationServiceTests`).

mod common;

use common::{TempDir, detect, inspect, make_plugin, make_product, make_product_of, make_product_with, make_theme};
use rd_model::{
    ProductType, ReleaseMode, SwiftDate, ValidationCheckId, ValidationReport, ValidationResult, ValidationStatus,
};
use rd_records::{
    AuthenticationAvailability, AutomationGate, ChangeKind, PreviewChange, ReleasePreview, ReleaseRecord,
    ReleaseSource, RepositorySnapshot,
};
use rd_validation::{ValidationContext, ValidationService, ValidationServicing};
use rd_versioning::ProductDetector;
use std::fs;

/// The arguments of the Swift `context(version:stableTag:current:tagExists:extra:product:snapshot:)`.
struct Ctx {
    version: &'static str,
    stable_tag: Option<&'static str>,
    current: Option<&'static str>,
    tag_exists: bool,
    extra: Vec<(&'static str, &'static str)>,
    product: rd_model::Product,
}

impl Default for Ctx {
    fn default() -> Self {
        Self {
            version: "1.1.0",
            stable_tag: None,
            current: Some("1.0.0"),
            tag_exists: false,
            extra: Vec::new(),
            product: make_product(),
        }
    }
}

impl Ctx {
    fn build(self) -> (ValidationContext, TempDir) {
        self.build_with(|_| {})
    }

    fn build_with(self, snapshot: impl FnOnce(&mut RepositorySnapshot)) -> (ValidationContext, TempDir) {
        let dir = TempDir::new();
        let root = make_plugin(&dir, self.version, self.stable_tag, &self.extra);
        let inspection = inspect(&root);
        // Swift: `try? ProductDetector().detect(...)`.
        let detected = ProductDetector::new()
            .detect(&root, Some(self.product.product_type), None, Some(&self.product.slug))
            .ok();
        let mut repo = RepositorySnapshot {
            svn_version: Some("1.14.5".into()),
            repository_exists: Some(true),
            current_version: self.current.map(str::to_owned),
            existing_versions: self.current.map(|c| vec![c.to_owned()]).unwrap_or_default(),
            release_path_exists: Some(self.tag_exists),
            working_copy_exists: true,
            working_copy_revision: Some(10),
            remote_revision: Some(10),
            authentication: AuthenticationAvailability::Keychain,
            ..RepositorySnapshot::default()
        };
        snapshot(&mut repo);
        (
            ValidationContext::new(self.product, inspection, detected, Some(repo)),
            dir,
        )
    }
}

fn validator() -> ValidationService {
    ValidationService::new()
}

fn failures(report: &ValidationReport, check: ValidationCheckId) -> Vec<&ValidationResult> {
    report
        .results
        .iter()
        .filter(|r| r.check == check && r.status == ValidationStatus::Failed)
        .collect()
}

fn preview(changes: Vec<PreviewChange>, fingerprint: &str) -> ReleasePreview {
    ReleasePreview {
        changes,
        diff_excerpt: String::new(),
        diff_truncated: false,
        commit_message: String::new(),
        fingerprint: fingerprint.into(),
        working_copy_revision: Some(1),
        created_at: SwiftDate::now(),
    }
}

#[test]
fn valid_release_passes() {
    let (ctx, _dir) = Ctx::default().build();
    let report = validator().validate(&ctx);
    assert!(!report.is_blocking(), "{:?}", report.blocking_results());
    use ValidationCheckId::*;
    for check in [
        SourceIntegrity,
        ProductIdentity,
        VersionValidity,
        VersionIncrease,
        MetadataValidity,
        RequiredFiles,
        PackageStructure,
        SvnConnectivity,
        WorkingCopyClean,
        ExistingTag,
        RemoteFreshness,
        Authentication,
    ] {
        assert!(
            report
                .results
                .iter()
                .any(|r| r.check == check && r.status == ValidationStatus::Passed),
            "{check:?} should pass"
        );
    }
    assert!(
        report
            .results
            .iter()
            .all(|r| !r.name.is_empty() && !r.message.is_empty())
    );
}

#[test]
fn same_version_is_blocked() {
    let (ctx, _dir) = Ctx {
        version: "1.0.0",
        current: Some("1.0.0"),
        ..Ctx::default()
    }
    .build();
    assert!(!failures(&validator().validate(&ctx), ValidationCheckId::VersionIncrease).is_empty());
}

#[test]
fn older_version_is_blocked() {
    let (ctx, _dir) = Ctx {
        version: "0.9.0",
        current: Some("1.0.0"),
        ..Ctx::default()
    }
    .build();
    assert!(!failures(&validator().validate(&ctx), ValidationCheckId::VersionIncrease).is_empty());
}

#[test]
fn invalid_version_requires_review() {
    let (ctx, _dir) = Ctx {
        version: "1.1-hotfix",
        ..Ctx::default()
    }
    .build();
    let report = validator().validate(&ctx);
    assert!(
        failures(&report, ValidationCheckId::VersionValidity)
            .first()
            .is_some_and(|r| r.message.contains("requires review"))
    );
    assert!(report.is_blocking());
}

#[test]
fn existing_tag_is_blocked() {
    let (ctx, _dir) = Ctx {
        tag_exists: true,
        ..Ctx::default()
    }
    .build();
    assert!(!failures(&validator().validate(&ctx), ValidationCheckId::ExistingTag).is_empty());
}

#[test]
fn stable_tag_mismatch_is_blocked() {
    let (ctx, _dir) = Ctx {
        version: "1.1.0",
        stable_tag: Some("1.0.0"),
        ..Ctx::default()
    }
    .build();
    let report = validator().validate(&ctx);
    let failed = failures(&report, ValidationCheckId::MetadataValidity);
    assert!(failed.iter().any(|r| r.message.contains("does not match")
        && r.file_path.as_deref() == Some("readme.txt")
        && r.suggestion.is_some()));
}

#[test]
fn stable_tag_trunk_is_blocked() {
    let (ctx, _dir) = Ctx {
        version: "1.1.0",
        stable_tag: Some("trunk"),
        ..Ctx::default()
    }
    .build();
    assert!(
        failures(&validator().validate(&ctx), ValidationCheckId::MetadataValidity)
            .iter()
            .any(|r| r.message.contains("trunk"))
    );
}

#[test]
fn missing_metadata_is_blocked() {
    let dir = TempDir::new();
    dir.write("p/sample-plugin.php", "<?php\n/* Plugin Name: Sample Plugin */\n");
    let inspection = inspect(&dir.path("p"));
    let detected = detect(&dir.path("p"), None, None);
    let repo = RepositorySnapshot {
        repository_exists: Some(true),
        release_path_exists: Some(false),
        authentication: AuthenticationAvailability::Keychain,
        ..RepositorySnapshot::default()
    };
    let report = validator().validate(&ValidationContext::new(
        make_product(),
        inspection,
        Some(detected),
        Some(repo),
    ));
    assert!(
        !failures(&report, ValidationCheckId::VersionValidity).is_empty(),
        "missing Version header"
    );
    assert!(
        failures(&report, ValidationCheckId::MetadataValidity)
            .iter()
            .any(|r| r.message.contains("readme.txt is missing"))
    );
}

#[test]
fn product_mismatch_is_blocked() {
    let mut product = make_product();
    product.expected_header_name = "Completely Different".into();
    let (ctx, _dir) = Ctx {
        product,
        ..Ctx::default()
    }
    .build();
    assert!(!failures(&validator().validate(&ctx), ValidationCheckId::ProductIdentity).is_empty());

    let (theme_ctx, _dir2) = Ctx {
        product: make_product_of(ProductType::Theme, "sample-plugin"),
        ..Ctx::default()
    }
    .build();
    assert!(
        validator().validate(&theme_ctx).is_blocking(),
        "a plugin package for a theme product must block"
    );
}

#[test]
fn dirty_working_copy_and_remote_changes_are_blocked() {
    let (dirty, _d1) = Ctx::default().build_with(|s| s.working_copy_changes = vec!["modified: trunk/x.php".into()]);
    assert!(!failures(&validator().validate(&dirty), ValidationCheckId::WorkingCopyClean).is_empty());
    let (stale, _d2) = Ctx::default().build_with(|s| s.out_of_date_paths = vec!["/wc/trunk/readme.txt".into()]);
    assert!(!failures(&validator().validate(&stale), ValidationCheckId::RemoteFreshness).is_empty());
}

#[test]
fn network_failure_and_missing_auth_block() {
    let (offline, _d1) = Ctx::default().build_with(|s| {
        *s = RepositorySnapshot {
            connectivity_error: Some("Unable to connect".into()),
            authentication: AuthenticationAvailability::Keychain,
            ..RepositorySnapshot::default()
        }
    });
    let report = validator().validate(&offline);
    assert!(!failures(&report, ValidationCheckId::SvnConnectivity).is_empty());
    assert!(
        report
            .blocking_results()
            .iter()
            .any(|r| r.check == ValidationCheckId::ExistingTag && r.status == ValidationStatus::Skipped),
        "skipped required checks block"
    );
    let (no_auth, _d2) = Ctx::default().build_with(|s| s.authentication = AuthenticationAvailability::None);
    assert!(!failures(&validator().validate(&no_auth), ValidationCheckId::Authentication).is_empty());
}

#[test]
fn forbidden_files_block() {
    let (ctx, _dir) = Ctx {
        extra: vec![("vendor/bundle.zip", "PK")],
        ..Ctx::default()
    }
    .build();
    assert!(
        failures(&validator().validate(&ctx), ValidationCheckId::ForbiddenFiles)
            .iter()
            .any(|r| r.file_path.as_deref() == Some("vendor/bundle.zip"))
    );
}

#[test]
fn warnings_block_when_configured() {
    let mut product = make_product();
    product.validation.block_on_warnings = true;
    let (ctx, _dir) = Ctx {
        version: "1.1.0-beta1",
        product,
        ..Ctx::default()
    }
    .build();
    assert!(
        validator().validate(&ctx).is_blocking(),
        "pre-release warning blocks with blockOnWarnings"
    );
}

#[test]
fn optional_checks_can_be_disabled_but_required_cannot() {
    let mut product = make_product();
    product.validation.disabled_checks = [ValidationCheckId::PackageSize, ValidationCheckId::VersionIncrease].into();
    let (ctx, _dir) = Ctx {
        version: "1.0.0",
        current: Some("1.0.0"),
        product,
        ..Ctx::default()
    }
    .build();
    let report = validator().validate(&ctx);
    assert!(!report.results.iter().any(|r| r.check == ValidationCheckId::PackageSize));
    assert!(!failures(&report, ValidationCheckId::VersionIncrease).is_empty());
}

#[test]
fn release_diff_validation() {
    let (mut ctx, _dir) = Ctx::default().build();
    ctx.preview = Some(preview(
        vec![PreviewChange::new("trunk/a.php", ChangeKind::Modified, false)],
        "x",
    ));
    assert!(
        failures(&validator().validate(&ctx), ValidationCheckId::ReleaseDiff)
            .iter()
            .any(|r| r.message.contains("does not create"))
    );
    ctx.preview.as_mut().unwrap().changes = vec![
        PreviewChange::copy("tags/1.1.0", ChangeKind::Added, true),
        PreviewChange::new("tags/0.5/x.php", ChangeKind::Modified, false),
    ];
    assert!(
        failures(&validator().validate(&ctx), ValidationCheckId::ReleaseDiff)
            .iter()
            .any(|r| r.file_path.as_deref() == Some("tags/0.5/x.php"))
    );
}

#[test]
fn theme_rules() {
    let dir = TempDir::new();
    let root = make_theme(&dir, "1.1.0-beta", "Sample Theme");
    dir.write("sample-theme/data.sql", "x");
    let product = make_product_of(ProductType::Theme, "sample-theme");
    let inspection = inspect(&root);
    let detected = detect(&root, None, None);
    let repo = RepositorySnapshot {
        repository_exists: Some(true),
        current_version: Some("1.0.0".into()),
        release_path_exists: Some(false),
        authentication: AuthenticationAvailability::Keychain,
        ..RepositorySnapshot::default()
    };
    let report = validator().validate(&ValidationContext::new(
        product.clone(),
        inspection,
        Some(detected.clone()),
        Some(repo),
    ));
    assert!(
        failures(&report, ValidationCheckId::VersionValidity)
            .iter()
            .any(|r| r.message.contains("only numbers"))
    );
    assert!(
        failures(&report, ValidationCheckId::ForbiddenFiles)
            .iter()
            .any(|r| r.file_path.as_deref() == Some("data.sql"))
    );

    fs::remove_file(root.join("screenshot.png")).unwrap();
    let inspection2 = inspect(&root);
    let report2 = validator().validate(&ValidationContext::new(product, inspection2, Some(detected), None));
    assert!(
        failures(&report2, ValidationCheckId::RequiredFiles)
            .iter()
            .any(|r| r.message.contains("screenshot"))
    );
}

#[test]
fn automation_gate() {
    let mut product = make_product_with(ProductType::Plugin, "sample-plugin", ReleaseMode::Automatic, false);
    let mut record = ReleaseRecord::new(&product, ReleaseSource::folder("/x"), ReleaseMode::Automatic);
    let (mut ctx, _dir) = Ctx::default().build();
    let preview = preview(vec![PreviewChange::copy("tags/1.1.0", ChangeKind::Added, true)], "fp");
    ctx.preview = Some(preview.clone());
    record.validation = Some(validator().validate(&ctx));
    record.repository = ctx.repository.clone();
    record.preview = Some(preview);
    assert!(
        AutomationGate::blocking_reasons(&product, &record, Some("fp"))
            .iter()
            .any(|r| r.contains("not explicitly enabled"))
    );
    product.automation_enabled = true;
    assert_eq!(
        AutomationGate::blocking_reasons(&product, &record, Some("fp")),
        Vec::<String>::new()
    );
    assert!(!AutomationGate::blocking_reasons(&product, &record, Some("changed")).is_empty());
    record.errors = vec!["x".into()];
    assert!(!AutomationGate::blocking_reasons(&product, &record, Some("fp")).is_empty());
}
