//! Ported from `RestoreReleaseTests.swift`: restoring an older version's code as a new,
//! higher version, against local file:// repositories.
#![allow(clippy::unwrap_used)]

mod common;

use common::{Harness, TempDir, fixtures, product, zip_folder};
use rd_model::SourceKind;
use rd_model::{Product, ProductType, ReleaseMode, ValidationCheckId, ValidationStatus};
use rd_pipeline::ReleaseError;
use rd_records::{ApprovalKind, ChangeKind, ReleaseKind, ReleaseSource, ReleaseState};
use rd_store::{ProductRepository, ReleaseRepository};

macro_rules! harness {
    () => {
        match Harness::new() {
            Some(harness) => harness,
            None => return,
        }
    };
}

/// Sample Plugin with 1.0.0 released, then 1.1.0 released with an extra (broken) file.
async fn plugin_with_two_releases(h: &Harness, require_clean_git_tree: bool) -> Product {
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
    product.release_mode = ReleaseMode::Assisted;
    product.validation.require_clean_git_tree = require_clean_git_tree;
    h.store.save_product(&product).unwrap();

    let src = TempDir::new();
    let plugin = fixtures::Plugin::new("1.1.0")
        .extra(&[("includes/broken.php", "<?php\n// broken\n")])
        .make(&src);
    let zip = h.work.path("sample-plugin-1.1.0.zip");
    zip_folder(&plugin, &zip);
    let mut record = h
        .pipeline
        .create_release(
            product.id,
            ReleaseSource::zip(zip.to_string_lossy()),
            Some(ReleaseMode::Assisted),
            ReleaseKind::Version,
            None,
        )
        .unwrap();
    h.pipeline.prepare(record.id).await.unwrap();
    h.pipeline.approve(record.id, ApprovalKind::User).await.unwrap();
    record = h.pipeline.publish(record.id).await.unwrap();
    assert_eq!(record.state, ReleaseState::Completed, "{:?}", record.errors);
    h.store.product(product.id).unwrap().unwrap()
}

fn remote(h: &Harness, path: &str) -> Result<String, String> {
    h.repo.run(&["cat", &format!("{}/sample-plugin/{path}", h.repo.url())])
}

#[tokio::test]
async fn options_list_older_versions_and_suggest_a_version_above_the_highest() {
    let h = harness!();
    let product = plugin_with_two_releases(&h, false).await;
    let options = h.pipeline.restore_options(product.id).await.unwrap();
    assert_eq!(options.current_version.as_deref(), Some("1.1.0"));
    assert_eq!(options.older_versions, ["1.0.0"]);
    assert_eq!(options.suggested_version.as_deref(), Some("1.1.1"));
}

#[tokio::test]
async fn restores_older_code_as_new_version_through_the_normal_release() {
    let h = harness!();
    let product = plugin_with_two_releases(&h, true).await;
    let mut record = h
        .pipeline
        .create_restore_release(
            product.id,
            "1.0.0",
            "1.1.1",
            "Restores the 1.0.0 code after a problem in 1.1.0.",
            Some(ReleaseMode::Assisted),
        )
        .await
        .unwrap();
    assert_eq!(record.restores_version.as_deref(), Some("1.0.0"));
    assert_eq!(record.source.kind(), SourceKind::Folder);

    record = h.pipeline.prepare(record.id).await.unwrap();
    assert_eq!(
        record.state,
        ReleaseState::Previewed,
        "{:?} {:?}",
        record.validation,
        record.errors
    );
    assert_eq!(record.version.as_deref(), Some("1.1.1"));
    assert_eq!(record.previous_version.as_deref(), Some("1.1.0"));
    assert!(
        record
            .validation
            .as_ref()
            .unwrap()
            .results
            .iter()
            .any(|r| r.check == ValidationCheckId::GitWorkingTree && r.status == ValidationStatus::Passed),
        "an exported tag has no Git tree"
    );
    let preview = record.preview.clone().unwrap();
    assert!(
        preview
            .changes
            .iter()
            .any(|c| c.path == "trunk/includes/broken.php" && c.kind == ChangeKind::Deleted)
    );
    assert!(preview.changes.iter().any(|c| c.path == "tags/1.1.1"));
    assert!(
        preview
            .commit_message
            .starts_with("Release Sample Plugin 1.1.1 (restores the 1.0.0 code)"),
        "{}",
        preview.commit_message
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

    let readme = remote(&h, "trunk/readme.txt").unwrap();
    assert!(readme.contains("Stable tag: 1.1.1"));
    assert!(readme.contains("= 1.1.1 - "));
    assert!(readme.contains("* Restores the 1.0.0 code after a problem in 1.1.0."));
    assert!(
        remote(&h, "tags/1.1.1/sample-plugin.php")
            .unwrap()
            .contains("Version: 1.1.1")
    );
    assert!(remote(&h, "tags/1.1.1/includes/broken.php").is_err());
    assert!(
        remote(&h, "tags/1.1.0/includes/broken.php").unwrap().contains("broken"),
        "released tags are never changed"
    );
    assert_eq!(
        h.store.product(product.id).unwrap().unwrap().current_version.as_deref(),
        Some("1.1.1")
    );
}

#[tokio::test]
async fn refuses_versions_that_would_not_update_sites() {
    let h = harness!();
    let product = plugin_with_two_releases(&h, false).await;
    let cases = [
        ("1.1.0", "1.1.1"), // not older than current
        ("0.5.0", "1.1.1"), // never released
        ("1.0.0", "1.1.0"), // not newer than the highest version
        ("1.0.0", "1.0.1"),
    ];
    for (old, new) in cases {
        match h.pipeline.create_restore_release(product.id, old, new, "", None).await {
            Err(ReleaseError::RestoreNotPossible(_)) => {}
            other => panic!("restoring {old} as {new} should be refused, got {other:?}"),
        }
    }
    let restores = h
        .store
        .releases(None)
        .unwrap()
        .into_iter()
        .filter(|r| r.restores_version.is_some())
        .count();
    assert_eq!(restores, 0);
}

#[tokio::test]
async fn restores_an_older_theme_version_into_a_new_version_directory() {
    let h = harness!();
    h.repo
        .create_theme("sample-theme", Some("1.0.0"), "Sample Theme")
        .unwrap();
    let src = TempDir::new();
    let theme = fixtures::make_theme(&src, "t", "1.1.0", "Sample Theme");
    src.write("t/broken.php", "<?php\n");
    h.repo
        .run(&[
            "import",
            "-m",
            "1.1.0",
            &theme.to_string_lossy(),
            &format!("{}/sample-theme/1.1.0", h.repo.url()),
        ])
        .unwrap();
    let mut theme_product = product(
        "Sample Theme",
        "sample-theme",
        ProductType::Theme,
        &format!("{}/sample-theme", h.repo.url()),
        &h.work.path("wc-theme"),
        Some("1.1.0"),
    );
    theme_product.release_mode = ReleaseMode::Assisted;
    h.store.save_product(&theme_product).unwrap();

    let options = h.pipeline.restore_options(theme_product.id).await.unwrap();
    assert_eq!(options.older_versions, ["1.0.0"]);
    let suggested = options.suggested_version.clone().unwrap();
    let mut record = h
        .pipeline
        .create_restore_release(theme_product.id, "1.0.0", &suggested, "", Some(ReleaseMode::Assisted))
        .await
        .unwrap();
    record = h.pipeline.prepare(record.id).await.unwrap();
    assert_eq!(
        record.state,
        ReleaseState::Previewed,
        "{:?} {:?}",
        record.validation,
        record.errors
    );
    assert_eq!(record.version.as_deref(), Some("1.1.1"));
    let changes = record.preview.clone().unwrap().changes;
    let paths: Vec<&str> = changes.iter().map(|c| c.path.as_str()).collect();
    assert!(changes.iter().all(|c| c.path.starts_with("1.1.1")), "{paths:?}");
    // The new directory is copied from 1.1.0 with history, then the restored code replaces it.
    assert!(
        changes
            .iter()
            .any(|c| c.path == "1.1.1/broken.php" && c.kind == ChangeKind::Deleted)
    );
}
