//! `LiveDryRunTests.testDryRunAgainstWordPressOrg` from `LiveRepositoryTests.swift`. Ignored by
//! default; it reads from plugins.svn.wordpress.org and never commits:
//!
//! ```sh
//! RELEASEDOCK_LIVE_DRYRUN_SLUG=tlp-team RELEASEDOCK_LIVE_DRYRUN_VERSION=6.0.8 \
//!   cargo test -p rd-pipeline --test live_repository -- --ignored --nocapture
//! ```
//!
//! Exports trunk into a temp folder, bumps the version there, and runs the full pipeline in
//! Dry Run mode with an isolated data folder and a fresh temporary checkout.
#![allow(clippy::unwrap_used)]

mod common;

use common::{StubGit, TempDir};
use rd_keychain::InMemoryKeychain;
use rd_model::{Product, ProductType, ReleaseMode, ValidationStatus};
use rd_pipeline::{PipelineDependencies, ReleasePipelineService, ReleaseStagingService};
use rd_records::{ReleaseKind, ReleaseSource, ReleaseState};
use rd_store::{AppPaths, ProductRepository, Store};
use rd_svn::{SvnService, SvnServicing};
use rd_versioning::ProductDetector;
use regex::Regex;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn replace(file: &Path, pattern: &str, template: &str) {
    let text = fs::read_to_string(file).unwrap();
    let regex = Regex::new(&format!("(?mi){pattern}")).unwrap();
    fs::write(file, regex.replace_all(&text, template).as_bytes()).unwrap();
}

#[tokio::test]
#[ignore = "reads plugins.svn.wordpress.org; set RELEASEDOCK_LIVE_DRYRUN_SLUG and RELEASEDOCK_LIVE_DRYRUN_VERSION"]
async fn dry_run_against_word_press_org() {
    let (Ok(slug), Ok(new_version)) = (
        std::env::var("RELEASEDOCK_LIVE_DRYRUN_SLUG"),
        std::env::var("RELEASEDOCK_LIVE_DRYRUN_VERSION"),
    ) else {
        eprintln!("skipped: set RELEASEDOCK_LIVE_DRYRUN_SLUG and RELEASEDOCK_LIVE_DRYRUN_VERSION");
        return;
    };
    let work = TempDir::new();
    let url = format!("https://plugins.svn.wordpress.org/{slug}");
    let svn = Arc::new(SvnService::new().with_command_timeout(Duration::from_secs(600)));
    let mut start = Instant::now();

    // 1. Package: current trunk with the version bumped (temp copy only).
    let package = work.path(&format!("package/{slug}"));
    svn.export(&format!("{url}/trunk"), &package).await.unwrap();
    let detected = ProductDetector::new()
        .detect(&package, Some(ProductType::Plugin), None, Some(&slug))
        .unwrap();
    let main = detected.main_file.clone().unwrap();
    let current = detected.version.clone().unwrap();
    replace(
        &package.join(&main),
        &format!(r"^([ \t/*#@]*Version:[ \t]*){}", regex::escape(&current)),
        &format!("${{1}}{new_version}"),
    );
    if let Some(readme) = &detected.readme_file {
        replace(
            &package.join(readme),
            r"^(Stable tag:[ \t]*).*$",
            &format!("${{1}}{new_version}"),
        );
    }
    println!("LIVE package: {slug} {current} → {new_version} ({:?})", start.elapsed());

    // 2. Pipeline in Dry Run mode, isolated data folder and fresh checkout.
    let store = Arc::new(Store::in_memory().unwrap());
    let paths = AppPaths::new(work.path("data"));
    paths.create_all().unwrap();
    let mut product = Product::new(
        detected.name.as_deref().unwrap_or(&slug),
        &slug,
        ProductType::Plugin,
        &url,
        &work.path("wc").to_string_lossy(),
    );
    product.release_mode = ReleaseMode::DryRun;
    store.save_product(&product).unwrap();
    let pipeline = ReleasePipelineService::new(
        PipelineDependencies::new(
            store.clone(),
            store.clone(),
            svn,
            Arc::new(StubGit::new()),
            Arc::new(InMemoryKeychain::new()),
            Arc::new(ReleaseStagingService::new(paths)),
        )
        .with_generator("live-dry-run"),
    );
    start = Instant::now();
    let mut record = pipeline
        .create_release(
            product.id,
            ReleaseSource::folder(package.to_string_lossy()),
            Some(ReleaseMode::DryRun),
            ReleaseKind::Version,
            None,
        )
        .unwrap();
    record = pipeline.prepare(record.id).await.unwrap();
    println!("LIVE dry run: {:?} in {:?}", record.state, start.elapsed());
    if let Some(repository) = &record.repository {
        println!(
            "LIVE current version on SVN: {} via {}",
            repository.current_version.as_deref().unwrap_or("?"),
            repository.current_version_source.as_deref().unwrap_or("?")
        );
    }
    for result in record.validation.iter().flat_map(|v| &v.results) {
        if result.status != ValidationStatus::Passed {
            println!("LIVE check {:?}: {} — {}", result.status, result.name, result.message);
        }
    }
    if let Some(preview) = &record.preview {
        println!(
            "LIVE preview: {} added, {} modified, {} deleted; {} entries",
            preview.added(),
            preview.modified(),
            preview.deleted(),
            preview.changes.len()
        );
        for change in preview.changes.iter().take(12) {
            let kind = if change.is_copy {
                "copy".to_owned()
            } else {
                format!("{:?}", change.kind)
            };
            println!("LIVE   {kind} {}", change.path);
        }
    }
    println!("LIVE errors: {:?}", record.errors);
    assert_eq!(record.state, ReleaseState::Completed, "dry run should complete");
    assert_eq!(record.committed_revision, None, "a dry run never commits");
}
