//! Ported from `ValidationServiceTests.testAutomationGate` (ValidationTests.swift) and
//! `ReadmeUpdateTests.testAutomationGateRequiresTheReleasedTag`.

mod common;

use common::make_product;
use rd_model::{ProductType, ReleaseMode, SwiftDate, ValidationCheckId, ValidationReport, ValidationResult};
use rd_records::{
    AuthenticationAvailability, AutomationGate, ChangeKind, PreviewChange, ReleaseKind, ReleasePreview, ReleaseRecord,
    ReleaseSource, RepositorySnapshot,
};

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

/// The report `ValidationService` produces for a valid 1.1.0 release over 1.0.0 with a
/// preview (the Swift test validates a fixture plugin; the validation service is ported
/// later, so the passing report is written out here).
fn passing_report() -> ValidationReport {
    use ValidationCheckId::*;
    ValidationReport {
        results: [
            SourceIntegrity,
            ProductIdentity,
            VersionValidity,
            VersionIncrease,
            MetadataValidity,
            RequiredFiles,
            ForbiddenFiles,
            PackageStructure,
            SvnConnectivity,
            WorkingCopyClean,
            ExistingTag,
            RemoteFreshness,
            ReleaseDiff,
            Authentication,
            ChangelogEntry,
        ]
        .into_iter()
        .map(|check| ValidationResult::pass(check, "ok"))
        .collect(),
        block_on_warnings: false,
    }
}

/// The snapshot built by `ValidationServiceTests.context()`.
fn context_snapshot() -> RepositorySnapshot {
    RepositorySnapshot {
        svn_version: Some("1.14.5".into()),
        repository_exists: Some(true),
        current_version: Some("1.0.0".into()),
        existing_versions: vec!["1.0.0".into()],
        release_path_exists: Some(false),
        working_copy_exists: true,
        working_copy_revision: Some(10),
        remote_revision: Some(10),
        authentication: AuthenticationAvailability::Keychain,
        ..RepositorySnapshot::default()
    }
}

#[test]
fn automation_gate() {
    let mut product = make_product(
        ProductType::Plugin,
        "sample-plugin",
        None,
        "/tmp/wc",
        ReleaseMode::Automatic,
        false,
    );
    let mut record = ReleaseRecord::new(&product, ReleaseSource::folder("/x"), ReleaseMode::Automatic);
    let preview = preview(vec![PreviewChange::copy("tags/1.1.0", ChangeKind::Added, true)], "fp");
    record.validation = Some(passing_report());
    record.repository = Some(context_snapshot());
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

#[test]
fn automation_gate_requires_the_released_tag() {
    // `registerPlugin(mode: .automatic)` registers Sample Plugin with current version 1.0.0.
    let mut product = make_product(
        ProductType::Plugin,
        "sample-plugin",
        Some("file:///tmp/repo/sample-plugin"),
        "/tmp/wc-plugin",
        ReleaseMode::Automatic,
        false,
    );
    product.current_version = Some("1.0.0".into());
    product.automation_enabled = true;
    let mut record = ReleaseRecord::new_with(
        rd_model::Id::new(),
        &product,
        ReleaseSource::folder("/tmp/x"),
        ReleaseMode::Automatic,
        ReleaseKind::ReadmeUpdate,
        SwiftDate::now(),
    );
    record.validation = Some(ValidationReport {
        results: vec![
            ValidationResult::pass(ValidationCheckId::VersionValidity, "ok"),
            ValidationResult::pass(ValidationCheckId::ReleasedVersion, "ok"),
            ValidationResult::pass(ValidationCheckId::ProductIdentity, "ok"),
        ],
        block_on_warnings: false,
    });
    record.preview = Some(preview(Vec::new(), "f"));
    record.repository = Some(RepositorySnapshot {
        release_path_exists: Some(true),
        authentication: AuthenticationAvailability::Keychain,
        ..RepositorySnapshot::default()
    });
    assert_eq!(
        AutomationGate::blocking_reasons(&product, &record, Some("f")),
        Vec::<String>::new()
    );
    record.repository.as_mut().unwrap().release_path_exists = Some(false);
    assert!(
        AutomationGate::blocking_reasons(&product, &record, Some("f"))
            .iter()
            .any(|r| r.contains("released tag"))
    );
}

// Rust-only: every reason, verbatim.

#[test]
fn lists_every_unmet_condition_verbatim() {
    let product = make_product(
        ProductType::Plugin,
        "sample-plugin",
        None,
        "/tmp/wc",
        ReleaseMode::Assisted,
        false,
    );
    let mut record = ReleaseRecord::new(&product, ReleaseSource::zip("/x.zip"), ReleaseMode::Automatic);
    assert_eq!(
        AutomationGate::blocking_reasons(&product, &record, None),
        [
            "Automatic publishing is not explicitly enabled for Sample Plugin",
            "The release has not been validated"
        ]
    );

    record.validation = Some(ValidationReport {
        results: vec![
            ValidationResult::fail(ValidationCheckId::VersionValidity, "bad"),
            ValidationResult::skip(ValidationCheckId::SvnConnectivity, "offline"),
            ValidationResult::skip(ValidationCheckId::ReleaseDiff, "later"),
        ],
        block_on_warnings: false,
    });
    record.repository = Some(RepositorySnapshot {
        working_copy_changes: vec!["M trunk/a.php".into()],
        out_of_date_paths: vec!["trunk".into()],
        ..RepositorySnapshot::default()
    });
    record.errors = vec!["boom".into()];
    assert_eq!(
        AutomationGate::blocking_reasons(&product, &record, None),
        [
            "Automatic publishing is not explicitly enabled for Sample Plugin",
            "The version is not a valid, final release version",
            "The version is not confirmed to be newer than the current release",
            "Product identity is not fully confirmed",
            "2 required check(s) did not pass",
            "Required checks were skipped: SVN connectivity, Release diff",
            "The working copy had local changes",
            "The working copy is not current with the repository",
            "The release tag is not confirmed absent",
            "No valid authentication is available",
            "No release preview exists",
            "The release has unresolved errors",
        ]
    );

    record.repository = None;
    record.preview = Some(preview(Vec::new(), "fp"));
    let reasons = AutomationGate::blocking_reasons(&product, &record, None);
    assert!(reasons.contains(&"Repository state is unknown".to_owned()));
    assert!(reasons.contains(&"The release preview changed".to_owned()));
}
