//! Ported from `PipelineIntegrationTests.swift`, plus the `PipelineIntegrationTests`
//! extensions in `CommitTemplateTests.swift` and `TranscriptTests.swift`.
//!
//! End-to-end release tests using the real svn CLI against local file:// repositories
//! laid out like WordPress.org. No network and no WordPress.org credentials are used.
//! Every test passes as skipped when svn/svnadmin are not installed.
#![allow(clippy::unwrap_used)]

mod common;

use common::{CommitFault, Harness, StubGit, TempDir, fixtures, product};
use rd_model::{ProductType, ReleaseMode, SwiftDate, ValidationCheckId, ValidationStatus};
use rd_pipeline::working_copy::local_changes;
use rd_pipeline::{ReleaseError, ReleasePipelineService};
use rd_records::{
    Approval, ApprovalKind, AuthenticationAvailability, ChangeKind, ConflictResolution, GitRepositoryState,
    ReleaseKind, ReleaseRecord, ReleaseSource, ReleaseState, ReleaseStateMachine,
};
use rd_store::{ProductRepository, ReleaseRepository};
use rd_svn::{SvnServicing, SvnTranscribing};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

macro_rules! harness {
    () => {
        match Harness::new() {
            Some(harness) => harness,
            None => return,
        }
    };
}

impl Harness {
    fn create(&self, product: &rd_model::Product, source: ReleaseSource) -> ReleaseRecord {
        self.pipeline
            .create_release(product.id, source, None, ReleaseKind::Version, None)
            .unwrap()
    }

    fn stored(&self, record: &ReleaseRecord) -> ReleaseRecord {
        self.store.release(record.id).unwrap().unwrap()
    }

    fn blocking_checks(record: &ReleaseRecord) -> Vec<ValidationCheckId> {
        record
            .validation
            .as_ref()
            .unwrap()
            .blocking_results()
            .iter()
            .map(|r| r.check)
            .collect()
    }

    /// Prepares a working copy whose trunk/readme.txt conflicts with a server change.
    async fn make_conflict(&self, product: &rd_model::Product) -> PathBuf {
        self.pipeline.update_working_copy(product.id).await.unwrap();
        let readme = product.working_copy_dir().join("trunk/readme.txt");
        fs::write(&readme, "local edit\n").unwrap();
        let other = self
            .work
            .path(&format!("other-wc-{}", &rd_model::Id::new().to_string()[..4]));
        self.repo
            .run(&[
                "checkout",
                &format!("{}/sample-plugin/trunk", self.repo.url()),
                &other.to_string_lossy(),
            ])
            .unwrap();
        fs::write(other.join("readme.txt"), "server edit\n").unwrap();
        self.repo
            .run(&["commit", "-m", "external", &other.to_string_lossy()])
            .unwrap();
        readme
    }
}

fn describe(record: &ReleaseRecord) -> String {
    format!(
        "{:?} {:?}",
        record.validation.as_ref().map(|v| v
            .blocking_results()
            .iter()
            .map(|r| r.message.clone())
            .collect::<Vec<_>>()),
        record.errors
    )
}

// MARK: Plugin

#[tokio::test]
async fn assisted_plugin_release_from_zip() {
    let h = harness!();
    let product = h.register_plugin(ReleaseMode::Assisted, false, Some("1.0.0"), true);
    let source = h.zip_source(
        "1.1.0",
        None,
        &[(".DS_Store", "x"), ("node_modules/a.js", "x"), ("logo@2x.png", "img")],
    );
    let mut record = h.create(&product, source);
    record = h.pipeline.prepare(record.id).await.unwrap();
    assert_eq!(record.state, ReleaseState::Previewed, "{}", describe(&record));
    assert_eq!(record.previous_version.as_deref(), Some("1.0.0"));
    let preview = record.preview.clone().unwrap();
    assert!(preview.changes.iter().any(|c| c.path == "tags/1.1.0" && c.is_copy));
    assert!(preview.changes.iter().any(|c| c.path == "trunk/logo@2x.png"));
    assert!(preview.changes.iter().any(|c| c.path == "assets/banner-772x250.png"));
    assert!(
        !preview
            .changes
            .iter()
            .any(|c| c.path.contains("node_modules") || c.path.contains(".DS_Store"))
    );
    assert!(preview.commit_message.contains(&record.commit_marker()));

    h.pipeline.approve(record.id, ApprovalKind::User).await.unwrap();
    record = h.pipeline.publish(record.id).await.unwrap();
    assert_eq!(record.state, ReleaseState::Completed, "{:?}", record.errors);
    let revision = record.committed_revision.unwrap();
    assert_eq!(
        record.verification.as_ref().map(|v| v.is_clean()),
        Some(true),
        "{:?}",
        record.verification
    );

    // Remote state
    let url = h.repo.url();
    let tag_files = h
        .repo
        .run(&["list", "-R", &format!("{url}/sample-plugin/tags/1.1.0")])
        .unwrap();
    assert!(tag_files.contains("logo@2x.png"));
    assert!(!tag_files.contains("node_modules"));
    let mime = h
        .repo
        .run(&[
            "propget",
            "svn:mime-type",
            &format!("{url}/sample-plugin/assets/banner-772x250.png"),
        ])
        .unwrap();
    assert_eq!(mime.trim(), "image/png");
    let log = h
        .repo
        .run(&["log", "-l", "1", &format!("{url}/sample-plugin")])
        .unwrap();
    assert!(log.contains(&format!("ReleaseDock-Release-ID: {}", record.id)));
    assert!(log.contains(&format!("r{revision}")));

    // Product, manifest, history
    let updated = h.store.product(product.id).unwrap().unwrap();
    assert_eq!(updated.current_version.as_deref(), Some("1.1.0"));
    assert_eq!(updated.last_successful_release.unwrap().revision, Some(revision));
    let manifest = fs::read_to_string(record.manifest_path.as_ref().unwrap()).unwrap();
    assert!(manifest.contains(&format!("\"finalRevision\" : {revision}")));
    assert!(manifest.contains("\"state\" : \"completed\""));
    let states: Vec<ReleaseState> = h.store.events(record.id).unwrap().iter().map(|t| t.to).collect();
    use ReleaseState::*;
    assert_eq!(
        states,
        [
            Detected, Inspected, Validated, Staged, Previewed, Approved, Publishing, Verifying, Completed
        ]
    );
    assert!(h.notifications.titles().iter().any(|t| t.contains("1.1.0 released")));
    assert_eq!(
        h.local_change_count(&product).await,
        0,
        "working copy is clean afterwards"
    );
    assert!(!h.paths.staging_directory(record.id).exists(), "staging cleaned up");
}

#[tokio::test]
async fn folder_release_removes_deleted_files_and_never_touches_source() {
    let h = harness!();
    let product = h.plugin();
    let source = h.folder_source("1.1.0", &[(".git/HEAD", "x"), ("new-file.php", "<?php")]);
    // Remove a file that exists in trunk.
    fs::remove_file(PathBuf::from(source.path()).join("includes/class-sample.php")).unwrap();
    let before = common::subpaths(&PathBuf::from(source.path()));
    let mut record = h.create(&product, source.clone());
    record = h.pipeline.prepare(record.id).await.unwrap();
    assert_eq!(record.state, ReleaseState::Previewed, "{}", describe(&record));
    let preview = record.preview.clone().unwrap();
    assert!(
        preview
            .changes
            .iter()
            .any(|c| c.path == "trunk/includes/class-sample.php" && c.kind == ChangeKind::Deleted)
    );
    assert!(
        preview
            .changes
            .iter()
            .any(|c| c.path == "trunk/new-file.php" && c.kind == ChangeKind::Added)
    );
    h.pipeline.approve(record.id, ApprovalKind::User).await.unwrap();
    record = h.pipeline.publish(record.id).await.unwrap();
    assert_eq!(record.state, ReleaseState::Completed);
    assert_eq!(
        common::subpaths(&PathBuf::from(source.path())),
        before,
        "source folder untouched"
    );
    let trunk = h
        .repo
        .run(&["list", "-R", &format!("{}/sample-plugin/trunk", h.repo.url())])
        .unwrap();
    assert!(!trunk.contains("class-sample.php"));
    assert!(!trunk.contains(".git"));
}

#[tokio::test]
async fn dry_run_commits_nothing() {
    let h = harness!();
    let product = h.register_plugin(ReleaseMode::DryRun, false, Some("1.0.0"), false);
    let base = format!("{}/sample-plugin", h.repo.url());
    let head = h.svn.info(&base).await.unwrap().last_changed_revision;
    let mut record = h.create(&product, h.zip("1.1.0"));
    record = h.pipeline.prepare(record.id).await.unwrap();
    assert_eq!(record.state, ReleaseState::Completed);
    assert_eq!(record.committed_revision, None);
    let after = h.svn.info(&base).await.unwrap().last_changed_revision;
    assert_eq!(after, head);
    assert_eq!(h.local_change_count(&product).await, 0);
    assert!(h.pipeline.approve(record.id, ApprovalKind::User).await.is_err());
}

#[tokio::test]
async fn same_version_and_older_version_fail_validation() {
    let h = harness!();
    let product = h.plugin();
    for version in ["1.0.0", "0.9.0"] {
        let mut record = h.create(&product, h.zip(version));
        record = h.pipeline.prepare(record.id).await.unwrap();
        assert_eq!(record.state, ReleaseState::Failed);
        assert!(
            Harness::blocking_checks(&record)
                .iter()
                .any(|c| *c == ValidationCheckId::VersionIncrease || *c == ValidationCheckId::ExistingTag)
        );
    }
    assert_eq!(h.local_change_count(&product).await, 0);
}

#[tokio::test]
async fn invalid_zip_fails() {
    let h = harness!();
    let product = h.plugin();
    let bad = h.work.write("bad.zip", "not a zip");
    let mut record = h.create(&product, ReleaseSource::zip(bad.to_string_lossy()));
    record = h.pipeline.prepare(record.id).await.unwrap();
    assert_eq!(record.state, ReleaseState::Failed);
    assert!(Harness::blocking_checks(&record).contains(&ValidationCheckId::SourceIntegrity));
}

#[tokio::test]
async fn existing_tag_blocks() {
    let h = harness!();
    let product = h.plugin();
    let url = h.repo.url();
    h.repo
        .run(&[
            "copy",
            "-m",
            "manual tag",
            &format!("{url}/sample-plugin/trunk"),
            &format!("{url}/sample-plugin/tags/1.1.0"),
        ])
        .unwrap();
    let mut record = h.create(&product, h.zip("1.1.0"));
    record = h.pipeline.prepare(record.id).await.unwrap();
    assert_eq!(record.state, ReleaseState::Failed);
    assert!(Harness::blocking_checks(&record).contains(&ValidationCheckId::ExistingTag));
}

#[tokio::test]
async fn dirty_working_copy_blocks_and_can_be_reverted_with_backup() {
    let h = harness!();
    let product = h.plugin();
    let mut first = h.create(&product, h.zip("1.1.0"));
    h.pipeline.prepare(first.id).await.unwrap();
    first = h.pipeline.cancel(first.id).await.unwrap();
    assert_eq!(first.state, ReleaseState::Cancelled);

    let wc = product.working_copy_dir();
    let readme = wc.join("trunk/readme.txt");
    fs::write(&readme, "local edit").unwrap();
    fs::write(wc.join("trunk/stray.txt"), "stray").unwrap();
    let mut record = h.create(&product, h.zip("1.1.0"));
    record = h.pipeline.prepare(record.id).await.unwrap();
    assert_eq!(record.state, ReleaseState::Failed);
    assert!(Harness::blocking_checks(&record).contains(&ValidationCheckId::WorkingCopyClean));
    assert_eq!(
        fs::read_to_string(&readme).unwrap(),
        "local edit",
        "a dirty working copy is never modified automatically"
    );

    let backup = h.pipeline.revert_working_copy(product.id).await.unwrap().unwrap();
    let backup_text = fs::read_to_string(&backup).unwrap();
    assert!(backup_text.contains("local edit"));
    assert!(backup.with_extension("files").join("trunk/stray.txt").exists());
    assert_eq!(h.local_change_count(&product).await, 0);
}

#[tokio::test]
async fn cancelling_an_untouched_staged_release_writes_no_backup() {
    let h = harness!();
    let product = h.plugin();
    let mut record = h.create(&product, h.zip("1.1.0"));
    h.pipeline.prepare(record.id).await.unwrap();
    record = h.pipeline.cancel(record.id).await.unwrap();
    assert_eq!(record.state, ReleaseState::Cancelled);
    let backups: Vec<String> = fs::read_dir(h.paths.backups())
        .map(|d| {
            d.map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    assert!(backups.is_empty(), "{backups:?}");
    let state = h.pipeline.working_copy_state(product.id).await.unwrap();
    assert!(state.is_clean(), "{:?}", state.changes);

    // A hand edit after the preview is the user's work: cancelling backs it up.
    record = h.create(&product, h.zip("1.1.0"));
    h.pipeline.prepare(record.id).await.unwrap();
    fs::write(
        product.working_copy_dir().join("trunk/includes/class-sample.php"),
        "hand edit",
    )
    .unwrap();
    h.pipeline.cancel(record.id).await.unwrap();
    assert!(
        fs::read_dir(h.paths.backups()).unwrap().any(|e| e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with("-cancelled.diff"))
    );
}

#[tokio::test]
async fn update_working_copy_reports_conflicts_and_server_resolution_backs_up() {
    let h = harness!();
    let product = h.plugin();
    let readme = h.make_conflict(&product).await;
    let state = h.pipeline.update_working_copy(product.id).await.unwrap();
    let conflicts: Vec<&str> = state.conflicts().iter().map(|c| c.path.as_str()).collect();
    assert_eq!(conflicts, ["trunk/readme.txt"]);

    let (resolved, backup) = h
        .pipeline
        .resolve_conflicts(product.id, &["trunk/readme.txt".into()], ConflictResolution::Server)
        .await
        .unwrap();
    assert!(resolved.is_clean(), "{:?}", resolved.changes);
    assert_eq!(fs::read_to_string(&readme).unwrap(), "server edit\n");
    let saved = backup.unwrap().with_extension("files").join("trunk/readme.txt");
    assert!(fs::read_to_string(saved).unwrap().contains("local edit"));
}

#[tokio::test]
async fn resolving_conflict_keeping_mine_leaves_a_local_change() {
    let h = harness!();
    let product = h.plugin();
    let readme = h.make_conflict(&product).await;
    h.pipeline.update_working_copy(product.id).await.unwrap();
    let (resolved, backup) = h
        .pipeline
        .resolve_conflicts(product.id, &["trunk/readme.txt".into()], ConflictResolution::Mine)
        .await
        .unwrap();
    assert_eq!(backup, None);
    assert!(resolved.conflicts().is_empty());
    let changes: Vec<&str> = resolved.changes.iter().map(|c| c.path.as_str()).collect();
    assert_eq!(changes, ["trunk/readme.txt"]);
    assert_eq!(fs::read_to_string(&readme).unwrap(), "local edit\n");
}

#[tokio::test]
async fn update_is_refused_while_a_release_is_staged() {
    let h = harness!();
    let product = h.plugin();
    let mut record = h.create(&product, h.zip("1.1.0"));
    record = h.pipeline.prepare(record.id).await.unwrap();
    assert_eq!(record.state, ReleaseState::Previewed);
    assert!(h.pipeline.update_working_copy(product.id).await.is_err());
}

#[tokio::test]
async fn remote_change_after_approval_blocks_publishing() {
    let h = harness!();
    let product = h.plugin();
    let mut record = h.create(&product, h.zip("1.1.0"));
    h.pipeline.prepare(record.id).await.unwrap();
    record = h.pipeline.approve(record.id, ApprovalKind::User).await.unwrap();
    // Someone commits to trunk in the meantime.
    let other = h.work.path("other-wc");
    let url = h.repo.url();
    h.repo
        .run(&[
            "checkout",
            &format!("{url}/sample-plugin/trunk"),
            &other.to_string_lossy(),
        ])
        .unwrap();
    fs::write(other.join("readme.txt"), "changed elsewhere").unwrap();
    h.repo
        .run(&["commit", "-m", "external", &other.to_string_lossy()])
        .unwrap();

    assert!(h.pipeline.publish(record.id).await.is_err());
    record = h.stored(&record);
    assert_eq!(record.state, ReleaseState::Failed);
    assert!(
        h.repo
            .run(&["info", &format!("{url}/sample-plugin/tags/1.1.0")])
            .is_err(),
        "nothing was published"
    );
    assert_eq!(h.svn.commit_count(), 0);
}

#[tokio::test]
async fn preview_change_after_approval_requires_reapproval() {
    let h = harness!();
    let product = h.plugin();
    let record = h.create(&product, h.zip("1.1.0"));
    h.pipeline.prepare(record.id).await.unwrap();
    h.pipeline.approve(record.id, ApprovalKind::User).await.unwrap();
    fs::write(
        product.working_copy_dir().join("trunk/includes/class-sample.php"),
        "tampered",
    )
    .unwrap();
    let error = h.pipeline.publish(record.id).await.unwrap_err();
    assert!(matches!(error, ReleaseError::PreviewChanged), "{error}");
    assert_eq!(h.stored(&record).state, ReleaseState::Previewed);
    assert_eq!(h.svn.commit_count(), 0);
}

#[tokio::test]
async fn authentication_failure_commits_nothing_and_allows_retry() {
    let h = harness!();
    let product = h.plugin();
    h.svn.set_commit_fault(CommitFault::AuthenticationFailure);
    let mut record = h.create(&product, h.zip("1.1.0"));
    h.pipeline.prepare(record.id).await.unwrap();
    h.pipeline.approve(record.id, ApprovalKind::User).await.unwrap();
    assert!(h.pipeline.publish(record.id).await.is_err());
    record = h.stored(&record);
    assert_eq!(record.state, ReleaseState::Previewed);
    assert_eq!(record.approval, None);

    h.svn.set_commit_fault(CommitFault::None);
    h.pipeline.approve(record.id, ApprovalKind::User).await.unwrap();
    record = h.pipeline.publish(record.id).await.unwrap();
    assert_eq!(record.state, ReleaseState::Completed);
}

#[tokio::test]
async fn missing_credentials_block_publishing() {
    let h = harness!();
    let mut product = h.plugin();
    // Simulate a remote https repository with no stored or cached credentials.
    h.svn.set_cached_credentials(false);
    product.svn_username = None;
    h.store.save_product(&product).unwrap();
    let remote = rd_model::Product::new(
        "x",
        "x",
        ProductType::Plugin,
        "https://plugins.svn.wordpress.org/x",
        "/tmp/x",
    );
    let auth = h.pipeline.authentication_availability(&remote).await;
    assert_eq!(auth, AuthenticationAvailability::None);
}

#[tokio::test]
async fn hook_rejection_fails_and_restores_working_copy() {
    let h = harness!();
    let product = h.plugin();
    h.svn.set_commit_fault(CommitFault::HookRejection);
    let record = h.create(&product, h.zip("1.1.0"));
    h.pipeline.prepare(record.id).await.unwrap();
    h.pipeline.approve(record.id, ApprovalKind::User).await.unwrap();
    assert!(h.pipeline.publish(record.id).await.is_err());
    assert_eq!(h.stored(&record).state, ReleaseState::Failed);
    assert_eq!(h.local_change_count(&product).await, 0);
}

#[tokio::test]
async fn commit_timeout_after_server_commit_is_recovered_without_recommitting() {
    let h = harness!();
    let product = h.plugin();
    h.svn.set_commit_fault(CommitFault::UncertainAfterCommit);
    let mut record = h.create(&product, h.zip("1.1.0"));
    h.pipeline.prepare(record.id).await.unwrap();
    h.pipeline.approve(record.id, ApprovalKind::User).await.unwrap();
    record = h.pipeline.publish(record.id).await.unwrap();
    assert_eq!(record.state, ReleaseState::Completed, "{:?}", record.errors);
    assert_eq!(h.svn.commit_count(), 1, "never retried blindly");
    assert!(record.committed_revision.is_some());
    let states: Vec<ReleaseState> = h.store.events(record.id).unwrap().iter().map(|t| t.to).collect();
    assert!(states.contains(&ReleaseState::RecoveryRequired));
    assert_eq!(h.local_change_count(&product).await, 0);
}

#[tokio::test]
async fn commit_timeout_without_server_commit_requires_explicit_retry() {
    let h = harness!();
    let product = h.plugin();
    h.svn.set_commit_fault(CommitFault::UncertainWithoutCommit);
    let mut record = h.create(&product, h.zip("1.1.0"));
    h.pipeline.prepare(record.id).await.unwrap();
    h.pipeline.approve(record.id, ApprovalKind::User).await.unwrap();
    record = h.pipeline.publish(record.id).await.unwrap();
    assert_eq!(record.state, ReleaseState::RecoveryRequired);
    assert!(record.verified_not_committed);
    assert_eq!(h.svn.commit_count(), 1);

    assert!(
        h.pipeline.approve(record.id, ApprovalKind::Automatic).await.is_err(),
        "automatic retry is never allowed"
    );
    h.svn.set_commit_fault(CommitFault::None);
    h.pipeline.approve(record.id, ApprovalKind::User).await.unwrap();
    record = h.pipeline.publish(record.id).await.unwrap();
    assert_eq!(record.state, ReleaseState::Completed);
    assert_eq!(h.svn.commit_count(), 2);
}

#[tokio::test]
async fn network_failure_during_commit_is_treated_as_uncertain() {
    let h = harness!();
    let product = h.plugin();
    h.svn.set_commit_fault(CommitFault::NetworkFailureBeforeCommit);
    let mut record = h.create(&product, h.zip("1.1.0"));
    h.pipeline.prepare(record.id).await.unwrap();
    h.pipeline.approve(record.id, ApprovalKind::User).await.unwrap();
    record = h.pipeline.publish(record.id).await.unwrap();
    assert_eq!(record.state, ReleaseState::RecoveryRequired);
    record = h.pipeline.abandon(record.id).await.unwrap();
    assert_eq!(record.state, ReleaseState::Failed);
    assert_eq!(h.local_change_count(&product).await, 0);
}

#[tokio::test]
async fn network_failure_during_validation() {
    let h = harness!();
    let product = h.plugin();
    h.svn.set_fail_info(true);
    let mut record = h.create(&product, h.zip("1.1.0"));
    record = h.pipeline.prepare(record.id).await.unwrap();
    assert_eq!(record.state, ReleaseState::Failed);
    assert!(Harness::blocking_checks(&record).contains(&ValidationCheckId::SvnConnectivity));
}

#[tokio::test]
async fn duplicate_release_is_rejected() {
    let h = harness!();
    let product = h.plugin();
    let mut first = h.create(&product, h.zip("1.1.0"));
    h.pipeline.prepare(first.id).await.unwrap();
    h.pipeline.approve(first.id, ApprovalKind::User).await.unwrap();
    first = h.pipeline.publish(first.id).await.unwrap();
    assert_eq!(first.state, ReleaseState::Completed);
    // Publishing the same version again.
    let mut second = h.create(&product, h.zip("1.1.0"));
    second = h.pipeline.prepare(second.id).await.unwrap();
    assert_eq!(second.state, ReleaseState::Failed);
    assert!(Harness::blocking_checks(&second).contains(&ValidationCheckId::ExistingTag));
    // Re-publishing a completed release is refused.
    assert!(h.pipeline.publish(first.id).await.is_err());
}

#[tokio::test]
async fn app_restart_during_release() {
    let h = harness!();
    let product = h.plugin();
    let mut staged = h.create(&product, h.zip("1.1.0"));
    staged = h.pipeline.prepare(staged.id).await.unwrap();
    assert!(staged.working_copy_modified);

    // Simulate a crash in the middle of publishing (state persisted, commit landed).
    let mut publishing = h.stored(&staged);
    let preview = publishing.preview.clone().unwrap();
    publishing.approval = Some(Approval::new(ApprovalKind::User, SwiftDate::now(), preview.fingerprint));
    ReleaseStateMachine::transition(&mut publishing, ReleaseState::Approved, None).unwrap();
    ReleaseStateMachine::transition(&mut publishing, ReleaseState::Publishing, None).unwrap();
    h.store.save_release(&publishing).unwrap();
    h.svn
        .commit(&[product.working_copy_dir()], &preview.commit_message, None)
        .await
        .unwrap();

    // "Restart": a fresh pipeline reconciles interrupted releases.
    let restarted = ReleasePipelineService::new(Harness::dependencies(
        &h.store,
        h.svn.clone(),
        &h.paths,
        Arc::new(StubGit::new()),
    ));
    let reconciled = restarted.reconcile_interrupted_releases().await.unwrap();
    let states: Vec<ReleaseState> = reconciled.iter().map(|r| r.state).collect();
    assert_eq!(states, [ReleaseState::RecoveryRequired]);
    let recovered = restarted.recover(staged.id).await.unwrap();
    assert_eq!(recovered.state, ReleaseState::Completed, "{:?}", recovered.errors);
    assert!(recovered.committed_revision.is_some());
}

#[tokio::test]
async fn restart_before_publishing_cancels_and_restores_working_copy() {
    let h = harness!();
    let product = h.plugin();
    let record = h.create(&product, h.zip("1.1.0"));
    h.pipeline.prepare(record.id).await.unwrap();
    assert!(h.local_change_count(&product).await > 0);
    let reconciled = h.pipeline.reconcile_interrupted_releases().await.unwrap();
    let states: Vec<ReleaseState> = reconciled.iter().map(|r| r.state).collect();
    assert_eq!(states, [ReleaseState::Cancelled]);
    assert_eq!(h.local_change_count(&product).await, 0);
}

#[tokio::test]
async fn automatic_release_requires_explicit_opt_in() {
    let h = harness!();
    let product = h.register_plugin(ReleaseMode::Automatic, false, Some("1.0.0"), false);
    let mut record = h.create(&product, h.zip("1.1.0"));
    h.pipeline.prepare(record.id).await.unwrap();
    let error = h.pipeline.publish_automatically(record.id).await.unwrap_err();
    assert!(matches!(error, ReleaseError::AutomationNotAuthorized(_)), "{error}");
    assert_eq!(h.svn.commit_count(), 0);

    let mut enabled = h.store.product(product.id).unwrap().unwrap();
    enabled.automation_enabled = true;
    h.store.save_product(&enabled).unwrap();
    record = h.pipeline.publish_automatically(record.id).await.unwrap();
    assert_eq!(record.state, ReleaseState::Completed);
    assert_eq!(record.approval.map(|a| a.kind), Some(ApprovalKind::Automatic));
}

#[tokio::test]
async fn first_release_into_empty_plugin_repository() {
    let h = harness!();
    let product = h.register_plugin(ReleaseMode::Assisted, false, None, false);
    let mut record = h.create(&product, h.zip("1.0.0"));
    record = h.pipeline.prepare(record.id).await.unwrap();
    assert_eq!(record.state, ReleaseState::Previewed, "{}", describe(&record));
    h.pipeline.approve(record.id, ApprovalKind::User).await.unwrap();
    record = h.pipeline.publish(record.id).await.unwrap();
    assert_eq!(record.state, ReleaseState::Completed);
}

#[tokio::test]
async fn file_and_directory_type_changes() {
    let h = harness!();
    let product = h.plugin();
    let src = TempDir::new();
    let root = fixtures::make_plugin(&src, "1.1.0");
    // includes/ (a directory in 1.0.0) becomes a file; assets/js/app.js (a file) becomes a directory.
    fs::remove_dir_all(root.join("includes")).unwrap();
    fs::write(root.join("includes"), "<?php // now a file\n").unwrap();
    fs::remove_file(root.join("assets/js/app.js")).unwrap();
    src.write("sample-plugin/assets/js/app.js/index.js", "console.log('dir');\n");

    let mut record = h.create(&product, ReleaseSource::folder(root.to_string_lossy()));
    record = h.pipeline.prepare(record.id).await.unwrap();
    assert_eq!(record.state, ReleaseState::Previewed, "{}", describe(&record));
    assert!(
        record
            .preview
            .as_ref()
            .unwrap()
            .changes
            .iter()
            .any(|c| c.path == "trunk/includes" && c.kind == ChangeKind::Replaced)
    );
    h.pipeline.approve(record.id, ApprovalKind::User).await.unwrap();
    record = h.pipeline.publish(record.id).await.unwrap();
    assert_eq!(record.state, ReleaseState::Completed, "{:?}", record.errors);
    assert_eq!(
        record.verification.as_ref().map(|v| v.is_clean()),
        Some(true),
        "{:?}",
        record.verification
    );
    let listing = h
        .repo
        .run(&["list", "-R", &format!("{}/sample-plugin/tags/1.1.0", h.repo.url())])
        .unwrap();
    assert!(listing.contains("assets/js/app.js/index.js"));
    assert!(listing.lines().any(|l| l == "includes"));
}

// MARK: Audit regressions

#[tokio::test]
async fn revert_backs_up_binary_and_added_files_before_deleting() {
    let h = harness!();
    let product = h.plugin();
    // Prime the working copy with a successful dry-run style preparation, then cancel.
    let first = h.create(&product, h.zip("1.1.0"));
    h.pipeline.prepare(first.id).await.unwrap();
    h.pipeline.cancel(first.id).await.unwrap();

    let wc = product.working_copy_dir();
    let binary: &[u8] = &[0x89, 0x50, 0x4E, 0x47, 0x00, 0xFF, 0x00, 0x10];
    fs::write(wc.join("trunk/new-image.png"), binary).unwrap();
    h.repo
        .run(&["add", &wc.join("trunk/new-image.png").to_string_lossy()])
        .unwrap();
    fs::write(wc.join("trunk/includes/class-sample.php"), binary).unwrap();

    let backup = h.pipeline.revert_working_copy(product.id).await.unwrap().unwrap();
    let files = backup.with_extension("files");
    assert_eq!(
        fs::read(files.join("trunk/new-image.png")).unwrap(),
        binary,
        "added binary file backed up"
    );
    assert_eq!(
        fs::read(files.join("trunk/includes/class-sample.php")).unwrap(),
        binary,
        "modified file backed up"
    );
    assert!(!wc.join("trunk/new-image.png").exists());
    let remaining = local_changes(&wc, h.svn.as_ref()).await.unwrap();
    assert!(remaining.is_empty(), "{remaining:?}");
}

#[tokio::test]
async fn ignored_files_in_working_copy_block_release_but_junk_does_not() {
    let h = harness!();
    let product = h.plugin();
    let warmup = h.create(&product, h.zip("1.1.0"));
    h.pipeline.prepare(warmup.id).await.unwrap();
    h.pipeline.cancel(warmup.id).await.unwrap();

    let wc = product.working_copy_dir();
    fs::write(wc.join("trunk/.DS_Store"), "junk").unwrap();
    let junk_only = local_changes(&wc, h.svn.as_ref()).await.unwrap();
    assert!(
        junk_only.is_empty(),
        "OS junk does not make the working copy dirty: {junk_only:?}"
    );
    fs::write(wc.join("trunk/library.a"), "compiled").unwrap(); // matches svn global-ignores
    let changes = local_changes(&wc, h.svn.as_ref()).await.unwrap();
    assert!(changes.iter().any(|c| c.contains("library.a")), "{changes:?}");
}

#[tokio::test]
async fn preview_change_after_approval_keeps_the_git_check_of_folder_releases() {
    let mut h = harness!();
    let mut product = h.plugin();
    product.validation.require_clean_git_tree = true;
    h.store.save_product(&product).unwrap();
    let clean = GitRepositoryState {
        is_repository: true,
        head_commit: Some("abc123".into()),
        branch: Some("main".into()),
        exact_tag: None,
        uncommitted_changes: Vec::new(),
        remote_url: None,
    };
    h.pipeline = Harness::make_pipeline(
        &h.store,
        &h.svn,
        &h.paths,
        Arc::new(StubGit::with_state(clean)),
        Some(h.notifications.clone()),
    );
    let mut record = h.create(&product, h.folder_source("1.1.0", &[]));
    record = h.pipeline.prepare(record.id).await.unwrap();
    assert_eq!(record.state, ReleaseState::Previewed, "{}", describe(&record));
    h.pipeline.approve(record.id, ApprovalKind::User).await.unwrap();
    fs::write(
        product.working_copy_dir().join("trunk/includes/class-sample.php"),
        "tampered",
    )
    .unwrap();
    assert!(h.pipeline.publish(record.id).await.is_err());
    record = h.stored(&record);
    assert_eq!(record.state, ReleaseState::Previewed, "{}", describe(&record));
    assert!(
        record
            .validation
            .as_ref()
            .unwrap()
            .results
            .iter()
            .any(|r| r.check == ValidationCheckId::GitWorkingTree && r.status == ValidationStatus::Passed)
    );
}

#[tokio::test]
async fn preview_change_after_approval_is_revalidated() {
    let h = harness!();
    let product = h.plugin();
    let mut record = h.create(&product, h.zip("1.1.0"));
    h.pipeline.prepare(record.id).await.unwrap();
    h.pipeline.approve(record.id, ApprovalKind::User).await.unwrap();
    // An edit outside the allowed release paths: modify the already released tag.
    let old_tag = product.working_copy_dir().join("tags/1.0.0");
    h.repo
        .run(&["update", "--set-depth", "infinity", &old_tag.to_string_lossy()])
        .unwrap();
    fs::write(old_tag.join("readme.txt"), "tampered").unwrap();
    assert!(h.pipeline.publish(record.id).await.is_err());
    record = h.stored(&record);
    assert_eq!(
        record.state,
        ReleaseState::Failed,
        "the changed preview fails release-diff validation"
    );
    assert!(Harness::blocking_checks(&record).contains(&ValidationCheckId::ReleaseDiff));
    assert_eq!(h.svn.commit_count(), 0);
}

#[tokio::test]
async fn verified_not_committed_is_cleared_on_retry() {
    let h = harness!();
    let product = h.plugin();
    h.svn.set_commit_fault(CommitFault::UncertainWithoutCommit);
    let mut record = h.create(&product, h.zip("1.1.0"));
    h.pipeline.prepare(record.id).await.unwrap();
    h.pipeline.approve(record.id, ApprovalKind::User).await.unwrap();
    record = h.pipeline.publish(record.id).await.unwrap();
    assert!(record.verified_not_committed);
    // Retry also ends uncertain, and the repository cannot be inspected this time.
    record = h.pipeline.approve(record.id, ApprovalKind::User).await.unwrap();
    assert!(
        !record.verified_not_committed,
        "a new attempt invalidates the earlier verification"
    );
    h.svn.set_commit_fault(CommitFault::UncertainAfterCommit);
    h.svn.set_fail_log(true);
    assert!(h.pipeline.publish(record.id).await.is_err());
    record = h.stored(&record);
    assert_eq!(record.state, ReleaseState::RecoveryRequired);
    assert!(!record.verified_not_committed);
    assert!(
        h.pipeline.abandon(record.id).await.is_err(),
        "cannot abandon an unverified outcome"
    );
    h.svn.set_fail_log(false);
    record = h.pipeline.recover(record.id).await.unwrap();
    assert_eq!(record.state, ReleaseState::Completed);
}

// MARK: Theme

#[tokio::test]
async fn theme_release_copies_previous_version() {
    let h = harness!();
    h.repo
        .create_theme("sample-theme", Some("1.0.0"), "Sample Theme")
        .unwrap();
    let theme_product = product(
        "Sample Theme",
        "sample-theme",
        ProductType::Theme,
        &format!("{}/sample-theme", h.repo.url()),
        &h.work.path("wc-theme"),
        None,
    );
    h.store.save_product(&theme_product).unwrap();
    let src = TempDir::new();
    let theme = fixtures::make_theme(&src, "sample-theme", "1.0.1", "Sample Theme");
    fs::remove_file(theme.join("functions.php")).unwrap();
    let mut record = h.create(&theme_product, ReleaseSource::folder(theme.to_string_lossy()));
    record = h.pipeline.prepare(record.id).await.unwrap();
    assert_eq!(record.state, ReleaseState::Previewed, "{}", describe(&record));
    let changes = &record.preview.as_ref().unwrap().changes;
    assert!(changes.iter().any(|c| c.path == "1.0.1" && c.is_copy));
    assert!(
        changes
            .iter()
            .all(|c| c.path == "1.0.1" || c.path.starts_with("1.0.1/"))
    );
    h.pipeline.approve(record.id, ApprovalKind::User).await.unwrap();
    record = h.pipeline.publish(record.id).await.unwrap();
    assert_eq!(record.state, ReleaseState::Completed, "{:?}", record.errors);
    assert_eq!(record.verification.as_ref().map(|v| v.is_clean()), Some(true));
    let url = h.repo.url();
    let listing = h.repo.run(&["list", &format!("{url}/sample-theme/1.0.1")]).unwrap();
    assert!(!listing.contains("functions.php"));
    let history = h
        .repo
        .run(&["log", "--stop-on-copy", "-v", &format!("{url}/sample-theme/1.0.1")])
        .unwrap();
    assert!(history.contains("(from /sample-theme/1.0.0"), "copied with history");
    assert!(!record.guidance.is_empty());
}

#[tokio::test]
async fn theme_without_existing_version_is_blocked() {
    let h = harness!();
    h.repo.create_theme("sample-theme", None, "Sample Theme").unwrap();
    let theme_product = product(
        "Sample Theme",
        "sample-theme",
        ProductType::Theme,
        &format!("{}/sample-theme", h.repo.url()),
        &h.work.path("wc-theme"),
        None,
    );
    h.store.save_product(&theme_product).unwrap();
    let src = TempDir::new();
    let theme = fixtures::make_theme(&src, "sample-theme", "1.0.0", "Sample Theme");
    let mut record = h.create(&theme_product, ReleaseSource::folder(theme.to_string_lossy()));
    record = h.pipeline.prepare(record.id).await.unwrap();
    assert_eq!(record.state, ReleaseState::Failed);
    assert!(
        record
            .validation
            .as_ref()
            .unwrap()
            .blocking_results()
            .iter()
            .any(|r| r.message.contains("upload") || r.message.contains("review"))
    );
}

// MARK: CommitTemplateTests.swift (pipeline tests)

#[tokio::test]
async fn template_becomes_the_summary_and_keeps_the_marker() {
    let h = harness!();
    let product = h.plugin();
    let mut record = h
        .pipeline
        .create_release(
            product.id,
            h.zip("1.1.0"),
            None,
            ReleaseKind::Version,
            Some("Tagging version {version} (from {previous})"),
        )
        .unwrap();
    record = h.pipeline.prepare(record.id).await.unwrap();
    let message = record.preview.as_ref().unwrap().commit_message.clone();
    assert!(
        message.starts_with("Tagging version 1.1.0 (from 1.0.0)\n\n"),
        "{message}"
    );
    assert!(
        message.contains(&record.commit_marker()),
        "recovery still finds the commit"
    );
}

#[tokio::test]
async fn invalid_template_is_refused_before_anything_runs() {
    let h = harness!();
    let product = h.plugin();
    let error = h
        .pipeline
        .create_release(
            product.id,
            h.zip("1.1.0"),
            None,
            ReleaseKind::Version,
            Some("Release {ver}"),
        )
        .unwrap_err();
    assert!(error.to_string().contains("{ver} is not a token"), "{error}");
}

// MARK: TranscriptTests.swift

#[tokio::test]
async fn release_records_its_svn_commands_per_operation() {
    let h = harness!();
    let product = h.plugin();
    let mut record = h.create(&product, h.zip("1.1.0"));
    h.pipeline.prepare(record.id).await.unwrap();
    h.pipeline.approve(record.id, ApprovalKind::User).await.unwrap();
    record = h.pipeline.publish(record.id).await.unwrap();
    assert_eq!(record.state, ReleaseState::Completed, "{:?}", record.errors);

    let entries = h.transcript.entries(product.id, Some(record.id), 1000);
    assert!(
        entries
            .iter()
            .any(|e| e.operation == "Prepare" && e.command.starts_with("svn status"))
    );
    let commit = entries.iter().find(|e| e.command.starts_with("svn commit")).unwrap();
    assert_eq!(commit.operation, "Publish");
    assert!(commit.succeeded());
    assert!(commit.output.contains("Committed revision"), "{}", commit.output);
    assert!(entries.iter().all(|e| e.release_id == Some(record.id)));
}

// MARK: Rust-only

/// The pipeline's futures can run on a multi-threaded runtime (Tauri commands).
#[test]
fn operations_are_send() {
    fn assert_send<T: Send>(_: &T) {}
    let Some(h) = Harness::new() else { return };
    let id = rd_model::Id::new();
    assert_send(&h.pipeline.prepare(id));
    assert_send(&h.pipeline.publish(id));
    assert_send(&h.pipeline.recover(id));
    assert_send(&h.pipeline.reconcile_interrupted_releases());
}

/// Two operations on one product never overlap: the second is refused at once.
#[tokio::test]
async fn a_busy_product_refuses_a_second_operation() {
    let h = harness!();
    let product = h.plugin();
    let first = h.create(&product, h.zip("1.1.0"));
    let second = h.create(&product, h.zip("1.1.0"));
    let (a, b) = tokio::join!(h.pipeline.prepare(first.id), h.pipeline.cancel(second.id));
    let refused = [a.as_ref().err(), b.as_ref().err()]
        .into_iter()
        .flatten()
        .any(|e| e.to_string() == "Another operation is already running for this product.");
    assert!(refused, "{:?} {:?}", a.map(|r| r.state), b.map(|r| r.state));
}

#[tokio::test]
async fn events_are_broadcast() {
    let h = harness!();
    let product = h.plugin();
    let mut events = h.pipeline.events();
    let record = h.create(&product, h.zip("1.1.0"));
    let event = events.recv().await.unwrap();
    assert_eq!(event.release_id, record.id);
    assert_eq!(event.state, ReleaseState::Detected);
    assert!(
        event.message.starts_with("Release detected from ZIP sample-plugin-"),
        "{}",
        event.message
    );
}
