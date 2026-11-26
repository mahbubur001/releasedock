//! Ported from `ReadmeUpdateTests.swift`: readme & assets updates of the released version,
//! against local file:// repositories. (`testAutomationGateRequiresTheReleasedTag` is pure
//! and lives in rd-records' `tests/automation_gate.rs`.)
#![allow(clippy::unwrap_used)]

mod common;

use common::{CommitFault, Harness, TempDir, fixtures, product, zip_folder};
use rd_model::{Id, Product, ProductType, ReleaseMode, ValidationCheckId, ValidationStatus};
use rd_pipeline::ReleaseError;
use rd_records::{ApprovalKind, ReleaseKind, ReleaseSource, ReleaseState};
use rd_store::{ProductRepository, ReleaseRepository};
use std::collections::BTreeSet;

macro_rules! harness {
    () => {
        match Harness::new() {
            Some(harness) => harness,
            None => return,
        }
    };
}

fn register_plugin(h: &Harness, mode: ReleaseMode, assets: bool) -> Product {
    h.repo
        .create_plugin("sample-plugin", Some("1.0.0"), "Sample Plugin")
        .unwrap();
    let mut product = product(
        "Sample Plugin",
        "sample-plugin",
        ProductType::Plugin,
        &format!("{}/sample-plugin", h.repo.url()),
        &h.work.path("wc-plugin"),
        Some("1.0.0"),
    );
    product.release_mode = mode;
    if assets {
        h.work.write("assets/banner-772x250.png", "PNG");
        product.assets_path = Some(h.work.path("assets").to_string_lossy().into_owned());
    }
    h.store.save_product(&product).unwrap();
    product
}

/// The released 1.0.0 package with "Tested up to" raised to 7.1, plus `extra` files.
fn source(h: &Harness, version: &str, extra: &[(&str, &str)]) -> ReleaseSource {
    let src = TempDir::new();
    let readme =
        fixtures::plugin_readme("Sample Plugin", version, None).replace("Tested up to: 6.8", "Tested up to: 7.1");
    let mut files: Vec<(&str, &str)> = extra.to_vec();
    files.push(("readme.txt", &readme));
    let plugin = fixtures::Plugin::new(version).extra(&files).make(&src);
    let zip = h
        .work
        .path(&format!("sample-plugin-{}.zip", &Id::new().to_string()[..4]));
    zip_folder(&plugin, &zip);
    ReleaseSource::zip(zip.to_string_lossy())
}

fn remote_readme(h: &Harness, path: &str) -> String {
    h.repo
        .run(&["cat", &format!("{}/sample-plugin/{path}/readme.txt", h.repo.url())])
        .unwrap()
}

fn readme_update(
    h: &Harness,
    product: &Product,
    source: ReleaseSource,
    mode: Option<ReleaseMode>,
) -> rd_records::ReleaseRecord {
    h.pipeline
        .create_release(product.id, source, mode, ReleaseKind::ReadmeUpdate, None)
        .unwrap()
}

#[tokio::test]
async fn updates_readme_in_trunk_and_released_tag_only() {
    let h = harness!();
    let product = register_plugin(&h, ReleaseMode::Assisted, true);
    let mut record = readme_update(&h, &product, source(&h, "1.0.0", &[]), None);
    record = h.pipeline.prepare(record.id).await.unwrap();
    assert_eq!(
        record.state,
        ReleaseState::Previewed,
        "{:?} {:?}",
        record.validation,
        record.errors
    );
    let results = &record.validation.as_ref().unwrap().results;
    assert!(
        results
            .iter()
            .any(|r| r.check == ValidationCheckId::ReleasedVersion && r.status == ValidationStatus::Passed)
    );
    assert!(!results.iter().any(|r| r.check == ValidationCheckId::VersionIncrease));
    let paths: BTreeSet<String> = record
        .preview
        .as_ref()
        .unwrap()
        .changes
        .iter()
        .map(|c| c.path.clone())
        .collect();
    let expected: BTreeSet<String> = ["trunk/readme.txt", "tags/1.0.0/readme.txt", "assets/banner-772x250.png"]
        .into_iter()
        .map(String::from)
        .collect();
    assert_eq!(paths, expected);
    assert!(
        record
            .preview
            .as_ref()
            .unwrap()
            .commit_message
            .starts_with("Update readme for Sample Plugin 1.0.0")
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
    assert!(record.committed_revision.is_some());

    assert!(remote_readme(&h, "trunk").contains("Tested up to: 7.1"));
    assert!(remote_readme(&h, "tags/1.0.0").contains("Tested up to: 7.1"));
    let tags = h
        .repo
        .run(&["list", &format!("{}/sample-plugin/tags", h.repo.url())])
        .unwrap();
    assert_eq!(tags.trim(), "1.0.0/", "no new tag");
    let updated = h.store.product(product.id).unwrap().unwrap();
    assert_eq!(updated.current_version.as_deref(), Some("1.0.0"));
    assert!(
        updated.last_successful_release.is_none(),
        "a readme update is not a version release"
    );
    assert!(record.guidance.iter().any(|g| g.contains("6 hours")));
    assert!(h.notifications.titles().iter().any(|t| t.contains("readme updated")));
    assert_eq!(h.local_change_count(&product).await, 0);
}

#[tokio::test]
async fn refuses_code_changes() {
    let h = harness!();
    let product = register_plugin(&h, ReleaseMode::Assisted, false);
    let record = readme_update(
        &h,
        &product,
        source(&h, "1.0.0", &[("includes/new.php", "<?php\n")]),
        None,
    );
    match h.pipeline.prepare(record.id).await {
        Err(ReleaseError::ReadmeUpdateChangesCode { tag, paths }) => {
            assert_eq!(tag, "tags/1.0.0");
            assert_eq!(paths, ["includes/new.php"]);
        }
        other => panic!("code changes must be refused, got {other:?}"),
    }
    assert_eq!(
        h.store.release(record.id).unwrap().map(|r| r.state),
        Some(ReleaseState::Failed)
    );
    assert_eq!(h.local_change_count(&product).await, 0, "working copy restored");
    assert_eq!(h.svn.commit_count(), 0);
}

#[tokio::test]
async fn requires_the_released_version() {
    let h = harness!();
    let product = register_plugin(&h, ReleaseMode::Assisted, false);
    let mut record = readme_update(&h, &product, source(&h, "1.1.0", &[]), None);
    record = h.pipeline.prepare(record.id).await.unwrap();
    assert_eq!(record.state, ReleaseState::Failed);
    let blocking = record.validation.as_ref().unwrap().blocking_results();
    assert!(
        blocking
            .iter()
            .any(|r| r.check == ValidationCheckId::ReleasedVersion && r.message.contains("1.1.0"))
    );
    assert!(
        blocking.iter().any(|r| r.check == ValidationCheckId::ExistingTag),
        "tags/1.1.0 does not exist"
    );
}

#[tokio::test]
async fn nothing_to_update_is_blocked() {
    let h = harness!();
    let product = register_plugin(&h, ReleaseMode::Assisted, false);
    let src = TempDir::new();
    let plugin = fixtures::make_plugin(&src, "1.0.0");
    let mut record = readme_update(&h, &product, ReleaseSource::folder(plugin.to_string_lossy()), None);
    match h.pipeline.prepare(record.id).await {
        Err(ReleaseError::ValidationBlocked(_)) => record = h.store.release(record.id).unwrap().unwrap(),
        other => panic!("an identical package must not produce an empty commit, got {other:?}"),
    }
    assert_eq!(record.state, ReleaseState::Failed);
    assert!(
        record
            .validation
            .as_ref()
            .unwrap()
            .blocking_results()
            .iter()
            .any(|r| r.check == ValidationCheckId::ReleaseDiff && r.message.contains("nothing to update"))
    );
}

#[tokio::test]
async fn dry_run_commits_nothing() {
    let h = harness!();
    let product = register_plugin(&h, ReleaseMode::Assisted, false);
    let mut record = readme_update(&h, &product, source(&h, "1.0.0", &[]), Some(ReleaseMode::DryRun));
    record = h.pipeline.prepare(record.id).await.unwrap();
    assert_eq!(record.state, ReleaseState::Completed, "{:?}", record.errors);
    assert_eq!(h.svn.commit_count(), 0);
    assert!(!remote_readme(&h, "tags/1.0.0").contains("Tested up to: 7.1"));
}

#[tokio::test]
async fn uncertain_commit_is_recovered_by_marker() {
    let h = harness!();
    let product = register_plugin(&h, ReleaseMode::Assisted, false);
    h.svn.set_commit_fault(CommitFault::UncertainAfterCommit);
    let mut record = readme_update(&h, &product, source(&h, "1.0.0", &[]), None);
    h.pipeline.prepare(record.id).await.unwrap();
    h.pipeline.approve(record.id, ApprovalKind::User).await.unwrap();
    record = h.pipeline.publish(record.id).await.unwrap();
    assert_eq!(record.state, ReleaseState::Completed, "{:?}", record.errors);
    assert_eq!(h.svn.commit_count(), 1, "never retried blindly");
    assert!(
        h.store
            .events(record.id)
            .unwrap()
            .iter()
            .any(|t| t.to == ReleaseState::RecoveryRequired)
    );
}

#[tokio::test]
async fn uncertain_commit_that_did_not_land_is_verified_absent() {
    let h = harness!();
    let product = register_plugin(&h, ReleaseMode::Assisted, false);
    h.svn.set_commit_fault(CommitFault::UncertainWithoutCommit);
    let mut record = readme_update(&h, &product, source(&h, "1.0.0", &[]), None);
    h.pipeline.prepare(record.id).await.unwrap();
    h.pipeline.approve(record.id, ApprovalKind::User).await.unwrap();
    record = h.pipeline.publish(record.id).await.unwrap();
    assert_eq!(
        record.state,
        ReleaseState::RecoveryRequired,
        "the existing tag must not be mistaken for someone else's release"
    );
    assert!(record.verified_not_committed);
    h.svn.set_commit_fault(CommitFault::None);
    h.pipeline.approve(record.id, ApprovalKind::User).await.unwrap();
    record = h.pipeline.publish(record.id).await.unwrap();
    assert_eq!(record.state, ReleaseState::Completed, "{:?}", record.errors);
    assert_eq!(h.svn.commit_count(), 2);
}

#[tokio::test]
async fn themes_cannot_use_readme_updates() {
    let h = harness!();
    h.repo
        .create_theme("sample-theme", Some("1.0.0"), "Sample Theme")
        .unwrap();
    let theme = product(
        "Sample Theme",
        "sample-theme",
        ProductType::Theme,
        &format!("{}/sample-theme", h.repo.url()),
        &h.work.path("wc-theme"),
        Some("1.0.0"),
    );
    h.store.save_product(&theme).unwrap();
    let src = TempDir::new();
    let root = fixtures::make_theme(&src, "sample-theme", "1.0.0", "Sample Theme");
    assert!(
        h.pipeline
            .create_release(
                theme.id,
                ReleaseSource::folder(root.to_string_lossy()),
                None,
                ReleaseKind::ReadmeUpdate,
                None
            )
            .is_err()
    );
}
