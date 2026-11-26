//! Added beyond the Swift suite. Swift covers these checks only through pipeline tests
//! (ReadmeUpdateTests, RestoreReleaseTests, PipelineIntegrationTests); these tests pin the
//! validation-level results and their exact messages.

mod common;

use common::{TempDir, detect, inspect, make_plugin, make_product, make_product_of, make_theme};
use rd_model::{
    InspectionIssue, InspectionIssueKind, ProductType, ReleaseMode, SwiftDate, ValidationCheckId, ValidationReport,
    ValidationResult, ValidationSeverity, ValidationStatus,
};
use rd_records::{
    AuthenticationAvailability, ChangeKind, GitRepositoryState, PreviewChange, ReleaseKind, ReleasePreview,
    RepositorySnapshot,
};
use rd_validation::{ValidationContext, ValidationService, ValidationServicing};

fn plugin_context(version: &str, repo: Option<RepositorySnapshot>) -> (ValidationContext, TempDir) {
    let dir = TempDir::new();
    let root = make_plugin(&dir, version, None, &[]);
    let inspection = inspect(&root);
    let detected = detect(&root, Some(ProductType::Plugin), Some("sample-plugin"));
    (
        ValidationContext::new(make_product(), inspection, Some(detected), repo),
        dir,
    )
}

fn released(current: Option<&str>, tag_exists: Option<bool>) -> RepositorySnapshot {
    RepositorySnapshot {
        repository_exists: Some(true),
        current_version: current.map(str::to_owned),
        release_path_exists: tag_exists,
        authentication: AuthenticationAvailability::Keychain,
        ..RepositorySnapshot::default()
    }
}

fn of(report: &ValidationReport, check: ValidationCheckId) -> Vec<(ValidationStatus, String)> {
    report
        .results
        .iter()
        .filter(|r| r.check == check)
        .map(|r| (r.status, r.message.clone()))
        .collect()
}

fn preview(changes: Vec<PreviewChange>) -> ReleasePreview {
    ReleasePreview {
        changes,
        diff_excerpt: String::new(),
        diff_truncated: false,
        commit_message: String::new(),
        fingerprint: "f".into(),
        working_copy_revision: Some(1),
        created_at: SwiftDate::now(),
    }
}

#[test]
fn readme_update_requires_the_released_version_and_its_tag() {
    let (mut ctx, _dir) = plugin_context("1.1.0", Some(released(Some("1.0.0"), Some(false))));
    ctx.kind = ReleaseKind::ReadmeUpdate;
    let report = ValidationService::new().validate(&ctx);
    assert_eq!(
        of(&report, ValidationCheckId::ReleasedVersion),
        [(
            ValidationStatus::Failed,
            "The package is version 1.1.0, but the released version is 1.0.0".into()
        )]
    );
    assert_eq!(
        of(&report, ValidationCheckId::ExistingTag),
        [(ValidationStatus::Failed, "tags/1.1.0 does not exist in SVN".into())]
    );
    assert!(
        of(&report, ValidationCheckId::VersionIncrease).is_empty(),
        "a readme update is not a new version"
    );

    let (mut same, _dir2) = plugin_context("1.0.0", Some(released(Some("1.0.0"), Some(true))));
    same.kind = ReleaseKind::ReadmeUpdate;
    let report = ValidationService::new().validate(&same);
    assert_eq!(
        of(&report, ValidationCheckId::ReleasedVersion),
        [(
            ValidationStatus::Passed,
            "Updates the readme of the released version 1.0.0".into()
        )]
    );
    assert_eq!(
        of(&report, ValidationCheckId::ExistingTag),
        [(ValidationStatus::Passed, "tags/1.0.0 exists and will be updated".into())]
    );
}

#[test]
fn readme_update_without_release_or_repository() {
    let (mut ctx, _dir) = plugin_context("1.0.0", Some(released(None, None)));
    ctx.kind = ReleaseKind::ReadmeUpdate;
    let report = ValidationService::new().validate(&ctx);
    assert_eq!(
        of(&report, ValidationCheckId::ReleasedVersion),
        [(
            ValidationStatus::Failed,
            "Sample Plugin has no released version to update".into()
        )]
    );
    ctx.repository = None;
    let report = ValidationService::new().validate(&ctx);
    assert_eq!(
        of(&report, ValidationCheckId::ReleasedVersion),
        [(
            ValidationStatus::Skipped,
            "Repository unavailable; the released version could not be checked".into()
        )]
    );
}

#[test]
fn themes_cannot_use_readme_updates() {
    let dir = TempDir::new();
    let root = make_theme(&dir, "1.0.0", "Lumen");
    let mut product = make_product_of(ProductType::Theme, "sample-theme");
    product.expected_header_name = "Lumen".into();
    let mut ctx = ValidationContext::new(
        product,
        inspect(&root),
        Some(detect(&root, None, None)),
        Some(released(Some("1.0.0"), Some(true))),
    );
    ctx.kind = ReleaseKind::ReadmeUpdate;
    let report = ValidationService::new().validate(&ctx);
    let result: Vec<&ValidationResult> = report
        .results
        .iter()
        .filter(|r| r.check == ValidationCheckId::ReleasedVersion)
        .collect();
    assert_eq!(result.len(), 1);
    assert_eq!(
        result[0].message,
        "Readme updates without a new version are only available for plugins"
    );
    assert_eq!(
        result[0].suggestion.as_deref(),
        Some("Publish a new theme version instead.")
    );
}

#[test]
fn readme_update_diff_may_only_touch_the_readme_and_assets() {
    let (mut ctx, _dir) = plugin_context("1.0.0", Some(released(Some("1.0.0"), Some(true))));
    ctx.kind = ReleaseKind::ReadmeUpdate;
    ctx.preview = Some(preview(Vec::new()));
    assert_eq!(
        of(&ValidationService::new().validate(&ctx), ValidationCheckId::ReleaseDiff),
        [(
            ValidationStatus::Failed,
            "The readme and assets already match the server; there is nothing to update".into()
        )]
    );
    ctx.preview = Some(preview(vec![
        PreviewChange::new("trunk/readme.txt", ChangeKind::Modified, false),
        PreviewChange::new("tags/1.0.0/readme.txt", ChangeKind::Modified, false),
        PreviewChange::new("assets/banner-772x250.png", ChangeKind::Added, false),
    ]));
    assert_eq!(
        of(&ValidationService::new().validate(&ctx), ValidationCheckId::ReleaseDiff),
        [(
            ValidationStatus::Passed,
            "3 change(s) to the readme and assets of tags/1.0.0".into()
        )]
    );
    ctx.preview.as_mut().unwrap().changes.push(PreviewChange::new(
        "trunk/sample-plugin.php",
        ChangeKind::Modified,
        false,
    ));
    let report = ValidationService::new().validate(&ctx);
    let failed: Vec<_> = report
        .results
        .iter()
        .filter(|r| r.check == ValidationCheckId::ReleaseDiff)
        .collect();
    assert_eq!(failed.len(), 1);
    assert_eq!(
        failed[0].message,
        "A readme update may only change the readme and /assets"
    );
    assert_eq!(failed[0].file_path.as_deref(), Some("trunk/sample-plugin.php"));
}

#[test]
fn release_diff_pass_message_counts_changes() {
    let (mut ctx, _dir) = plugin_context("1.1.0", Some(released(Some("1.0.0"), Some(false))));
    ctx.preview = Some(preview(vec![
        PreviewChange::new("trunk/a.php", ChangeKind::Modified, false),
        PreviewChange::new("trunk/b.php", ChangeKind::Added, false),
        PreviewChange::new("trunk/old.php", ChangeKind::Deleted, false),
        PreviewChange::copy("tags/1.1.0", ChangeKind::Added, true),
    ]));
    assert_eq!(
        of(&ValidationService::new().validate(&ctx), ValidationCheckId::ReleaseDiff),
        [(
            ValidationStatus::Passed,
            "1 added, 1 modified, 1 deleted; creates tags/1.1.0".into()
        )]
    );
}

#[test]
fn git_tree_and_restored_versions() {
    let (mut ctx, _dir) = plugin_context("1.1.0", Some(released(Some("1.0.0"), Some(false))));
    assert!(
        of(
            &ValidationService::new().validate(&ctx),
            ValidationCheckId::GitWorkingTree
        )
        .is_empty(),
        "no Git state and no requirement"
    );
    ctx.product.validation.require_clean_git_tree = true;
    assert_eq!(
        of(
            &ValidationService::new().validate(&ctx),
            ValidationCheckId::GitWorkingTree
        ),
        [(
            ValidationStatus::Failed,
            "A clean Git tree is required but the folder is not a Git repository".into()
        )]
    );
    let mut git = GitRepositoryState::not_a_repository();
    git.is_repository = true;
    git.head_commit = Some("0123456789abcdef".into());
    ctx.git = Some(git.clone());
    assert_eq!(
        of(
            &ValidationService::new().validate(&ctx),
            ValidationCheckId::GitWorkingTree
        ),
        [(ValidationStatus::Passed, "Git tree clean at 0123456789".into())]
    );
    git.uncommitted_changes = vec![" M a.php".into(), "?? b.php".into()];
    ctx.git = Some(git);
    assert_eq!(
        of(
            &ValidationService::new().validate(&ctx),
            ValidationCheckId::GitWorkingTree
        ),
        [(
            ValidationStatus::Failed,
            "Development folder has 2 uncommitted change(s)".into()
        )]
    );
    ctx.product.validation.require_clean_git_tree = false;
    assert_eq!(
        of(
            &ValidationService::new().validate(&ctx),
            ValidationCheckId::GitWorkingTree
        )[0]
        .0,
        ValidationStatus::Warning
    );
    ctx.restores_version = Some("1.0.0".into());
    assert_eq!(
        of(
            &ValidationService::new().validate(&ctx),
            ValidationCheckId::GitWorkingTree
        ),
        [(
            ValidationStatus::Passed,
            "Code exported from the released 1.0.0 on WordPress.org".into()
        )]
    );
}

#[test]
fn repository_messages() {
    let repo = RepositorySnapshot {
        svn_version: Some("1.14.5".into()),
        repository_exists: Some(true),
        current_version: Some("1.0.0".into()),
        release_path_exists: Some(false),
        working_copy_exists: true,
        working_copy_revision: Some(10),
        remote_revision: Some(12),
        authentication: AuthenticationAvailability::CachedBySvn,
        ..RepositorySnapshot::default()
    };
    let (mut ctx, _dir) = plugin_context("1.1.0", Some(repo));
    let report = ValidationService::new().validate(&ctx);
    assert_eq!(
        of(&report, ValidationCheckId::SvnConnectivity),
        [(ValidationStatus::Passed, "Connected (svn 1.14.5, r12)".into())]
    );
    assert_eq!(
        of(&report, ValidationCheckId::RemoteFreshness),
        [(ValidationStatus::Passed, "Working copy is up to date (r10)".into())]
    );
    assert_eq!(
        of(&report, ValidationCheckId::Authentication),
        [(
            ValidationStatus::Passed,
            "svn has cached credentials for this repository".into()
        )]
    );
    assert_eq!(
        of(&report, ValidationCheckId::VersionIncrease),
        [(ValidationStatus::Passed, "1.1.0 is newer than 1.0.0".into())]
    );
    assert_eq!(
        of(&report, ValidationCheckId::ExistingTag),
        [(ValidationStatus::Passed, "tags/1.1.0 does not exist yet".into())]
    );

    let repo = ctx.repository.as_mut().unwrap();
    repo.repository_exists = Some(false);
    repo.authentication = AuthenticationAvailability::None;
    repo.working_copy_changes = (1..=7).map(|i| format!("M f{i}.php")).collect();
    ctx.product.release_mode = ReleaseMode::DryRun;
    let report = ValidationService::new().validate(&ctx);
    assert_eq!(
        of(&report, ValidationCheckId::SvnConnectivity),
        [(
            ValidationStatus::Failed,
            "Repository https://plugins.svn.wordpress.org/sample-plugin does not exist".into()
        )]
    );
    assert_eq!(
        of(&report, ValidationCheckId::WorkingCopyClean),
        [(
            ValidationStatus::Failed,
            "Working copy has 7 local change(s): M f1.php, M f2.php, M f3.php, M f4.php, M f5.php".into()
        )]
    );
    assert_eq!(
        of(&report, ValidationCheckId::Authentication),
        [(
            ValidationStatus::Warning,
            "No SVN credentials are available; a real release will need them".into()
        )]
    );

    ctx.repository = None;
    let report = ValidationService::new().validate(&ctx);
    for check in [
        ValidationCheckId::SvnConnectivity,
        ValidationCheckId::WorkingCopyClean,
        ValidationCheckId::RemoteFreshness,
        ValidationCheckId::Authentication,
    ] {
        assert_eq!(
            of(&report, check),
            [(ValidationStatus::Skipped, "Repository not checked".into())]
        );
    }
}

#[test]
fn source_structure_and_size_messages() {
    let (mut ctx, _dir) = plugin_context("1.1.0", Some(released(Some("1.0.0"), Some(false))));
    let report = ValidationService::new().validate(&ctx);
    let source = of(&report, ValidationCheckId::SourceIntegrity);
    assert_eq!(source.len(), 1);
    assert!(source[0].1.starts_with("Folder digest "), "{}", source[0].1);
    assert!(source[0].1.ends_with("…, 4 files"), "{}", source[0].1);
    assert_eq!(
        of(&report, ValidationCheckId::PackageStructure),
        [(ValidationStatus::Passed, "Package structure is safe".into())]
    );
    let size = of(&report, ValidationCheckId::PackageSize);
    assert!(
        size[0].1.starts_with("Package size ") && size[0].1.ends_with(" bytes"),
        "{size:?}"
    );

    ctx.inspection.total_uncompressed_size = (12 << 20) + 5;
    ctx.inspection.issues.push(InspectionIssue {
        kind: InspectionIssueKind::NestedRoot,
        severity: ValidationSeverity::Error,
        path: Some("sample-plugin/sample-plugin".into()),
        message: "Nested root".into(),
        excluded: false,
    });
    let report = ValidationService::new().validate(&ctx);
    assert_eq!(
        of(&report, ValidationCheckId::PackageSize),
        [(
            ValidationStatus::Warning,
            "Package is 12 MiB (warning threshold 10 MiB)".into()
        )]
    );
    let structure: Vec<_> = report
        .results
        .iter()
        .filter(|r| r.check == ValidationCheckId::PackageStructure)
        .collect();
    assert_eq!(structure.len(), 1);
    assert_eq!(structure[0].status, ValidationStatus::Failed);
    assert_eq!(
        structure[0].suggestion.as_deref(),
        Some("Rebuild the ZIP so the plugin or theme files sit directly inside one top-level folder.")
    );

    ctx.inspection.issues.push(InspectionIssue {
        kind: InspectionIssueKind::CorruptArchive,
        severity: ValidationSeverity::Error,
        path: None,
        message: "The ZIP is damaged".into(),
        excluded: false,
    });
    assert_eq!(
        of(
            &ValidationService::new().validate(&ctx),
            ValidationCheckId::SourceIntegrity
        ),
        [(ValidationStatus::Failed, "The ZIP is damaged".into())]
    );
}

#[test]
fn excluded_paths_are_counted_in_the_forbidden_files_pass() {
    let (mut ctx, _dir) = plugin_context("1.1.0", Some(released(Some("1.0.0"), Some(false))));
    for (kind, path) in [
        (InspectionIssueKind::DevelopmentArtifact, "node_modules"),
        (InspectionIssueKind::JunkFile, ".DS_Store"),
    ] {
        ctx.inspection.issues.push(InspectionIssue {
            kind,
            severity: ValidationSeverity::Info,
            path: Some(path.into()),
            message: "excluded".into(),
            excluded: true,
        });
    }
    assert_eq!(
        of(
            &ValidationService::new().validate(&ctx),
            ValidationCheckId::ForbiddenFiles
        ),
        [(
            ValidationStatus::Passed,
            "No forbidden files in the release candidate; 2 path(s) excluded (1 development or VCS artifacts)".into()
        )]
    );
}

#[test]
fn undetected_product_fails_identity_and_metadata_with_the_detection_error() {
    let (mut ctx, _dir) = plugin_context("1.1.0", Some(released(Some("1.0.0"), Some(false))));
    ctx.detected = None;
    ctx.detection_error = Some("No plugin header found".into());
    let report = ValidationService::new().validate(&ctx);
    let identity: Vec<_> = report
        .results
        .iter()
        .filter(|r| r.check == ValidationCheckId::ProductIdentity)
        .collect();
    assert_eq!(identity[0].message, "No plugin was detected in the package");
    assert_eq!(identity[0].suggestion.as_deref(), Some("No plugin header found"));
    assert_eq!(
        of(&report, ValidationCheckId::MetadataValidity),
        [(ValidationStatus::Failed, "Product metadata could not be read".into())]
    );
    assert_eq!(
        of(&report, ValidationCheckId::RequiredFiles),
        [(
            ValidationStatus::Skipped,
            "Skipped because the product was not detected".into()
        )]
    );
    assert_eq!(
        of(&report, ValidationCheckId::VersionValidity),
        [(ValidationStatus::Failed, "No Version header found".into())]
    );
}

#[test]
fn names_are_normalized_before_comparing() {
    assert_eq!(
        ValidationService::normalize_name("  Foo &amp;\n Bar\tBAZ "),
        "foo & bar baz"
    );
}
