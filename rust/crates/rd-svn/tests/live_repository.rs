//! SVN-level part of LiveRepositoryTests.swift. Opt-in, read-only checks against
//! WordPress.org and a local checkout; they contact the network, so they are ignored:
//!
//! ```sh
//! RELEASEDOCK_LIVE_SLUG=tlp-team RELEASEDOCK_LIVE_WC=~/svn/tlp-team \
//!   cargo test -p rd-svn --test live_repository -- --ignored --nocapture
//! ```
//!
//! The Swift tests go through `RepositorySetupService` (describeRemote /
//! describeWorkingCopyLocally) and the release pipeline; these check the svn calls those
//! services make. The full versions move to the crate that ports `RepositorySetupService`.

mod support;

use rd_svn::*;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use support::TempDir;

fn expand_tilde(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => PathBuf::from(rd_svn::process_runner::home_directory()).join(rest),
        None => PathBuf::from(path),
    }
}

#[tokio::test]
#[ignore = "contacts plugins.svn.wordpress.org; set RELEASEDOCK_LIVE_SLUG"]
async fn describe_real_repository_and_checkout() {
    let Ok(slug) = std::env::var("RELEASEDOCK_LIVE_SLUG") else {
        eprintln!("skipped: set RELEASEDOCK_LIVE_SLUG");
        return;
    };
    let svn = SvnService::new();
    let url = format!("https://plugins.svn.wordpress.org/{slug}");

    let start = Instant::now();
    let info = svn.info(&url).await.unwrap();
    let tags = svn.list(&format!("{url}/tags"), false).await.unwrap();
    let readme = String::from_utf8_lossy(&svn.cat(&format!("{url}/trunk/readme.txt")).await.unwrap()).into_owned();
    let log = svn.log(&url, 5).await.unwrap();
    let stable = readme
        .lines()
        .find_map(|l| l.strip_prefix("Stable tag:"))
        .map(str::trim)
        .map(str::to_owned);
    println!(
        "LIVE remote: {:?} r{} tags={} stable={stable:?} last={:?}",
        start.elapsed(),
        info.revision,
        tags.len(),
        log.first().map(|e| e.revision)
    );
    assert_eq!(info.kind, SvnNodeKind::Dir);
    assert!(stable.is_some(), "readme has a Stable tag");
    assert!(!log.is_empty());
    assert!(svn.exists(&format!("{url}/trunk")).await.unwrap());
    assert!(
        !svn.exists(&format!("{url}/tags/0.0.0-releasedock-missing"))
            .await
            .unwrap()
    );

    if let Ok(wc) = std::env::var("RELEASEDOCK_LIVE_WC") {
        let wc = expand_tilde(&wc);
        let start = Instant::now();
        let local = svn.info(&wc.to_string_lossy()).await.unwrap();
        let changes = svn.status(&wc, false).await.unwrap();
        println!(
            "LIVE local: {:?} r{} changes={}",
            start.elapsed(),
            local.revision,
            changes.len()
        );
        assert!(start.elapsed() < Duration::from_secs(3));
    }
}

/// Step 1 of LiveDryRunTests.swift (export trunk into a temp folder); the dry-run pipeline
/// itself is not ported yet.
#[tokio::test]
#[ignore = "contacts plugins.svn.wordpress.org; set RELEASEDOCK_LIVE_DRYRUN_SLUG"]
async fn export_trunk_from_word_press_org() {
    let Ok(slug) = std::env::var("RELEASEDOCK_LIVE_DRYRUN_SLUG") else {
        eprintln!("skipped: set RELEASEDOCK_LIVE_DRYRUN_SLUG");
        return;
    };
    let work = TempDir::new();
    let package = work.path(&format!("package/{slug}"));
    let svn = SvnService::new().with_command_timeout(Duration::from_secs(600));
    let start = Instant::now();
    svn.export(&format!("https://plugins.svn.wordpress.org/{slug}/trunk"), &package)
        .await
        .unwrap();
    println!("LIVE export: {slug} in {:?}", start.elapsed());
    assert!(package.join("readme.txt").exists());
    assert!(!package.join(".svn").exists());
}
