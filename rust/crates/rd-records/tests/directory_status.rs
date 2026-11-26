//! Ported from the pure tests in `DirectoryStatusTests.swift`.

use rd_model::{Id, Product, ProductType, ReleaseMode, SwiftDate};
use rd_records::{
    DIRECTORY_WAIT_WINDOW, DirectoryListing, DirectoryState, DirectoryStatus, ReleaseKind, ReleaseRecord,
    ReleaseSource, ReleaseState, StateTransition,
};

fn listing(version: &str) -> DirectoryListing {
    DirectoryListing::new(Some(version.into()))
}

#[test]
fn states() {
    assert_eq!(
        DirectoryStatus::state("2.4.0", Some(&listing("2.4.0"))),
        DirectoryState::Live
    );
    assert_eq!(
        DirectoryStatus::state("2.4.0", Some(&listing("2.3.0"))),
        DirectoryState::Pending
    );
    assert_eq!(
        DirectoryStatus::state("2.4.0", Some(&listing("2.5.0"))),
        DirectoryState::Superseded
    );
    assert_eq!(DirectoryStatus::state("2.4.0", None), DirectoryState::NotListed);
    // Rust-only: a listing without a version is still pending.
    assert_eq!(
        DirectoryStatus::state("2.4.0", Some(&DirectoryListing::new(None))),
        DirectoryState::Pending
    );
}

fn product() -> Product {
    Product::new(
        "Acme Gallery",
        "acme-gallery",
        ProductType::Plugin,
        "https://plugins.svn.wordpress.org/acme-gallery/",
        "/tmp/wc-acme",
    )
}

struct Options {
    url: &'static str,
    mode: ReleaseMode,
    kind: ReleaseKind,
    completed_at: SwiftDate,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            url: "https://plugins.svn.wordpress.org/acme-gallery/",
            mode: ReleaseMode::Assisted,
            kind: ReleaseKind::Version,
            completed_at: SwiftDate::now(),
        }
    }
}

fn record(options: Options) -> ReleaseRecord {
    let mut product = product();
    product.svn_repository_url = options.url.into();
    let mut record = ReleaseRecord::new_with(
        Id::new(),
        &product,
        ReleaseSource::zip("/tmp/acme.zip"),
        options.mode,
        options.kind,
        options.completed_at,
    );
    record.version = Some("2.4.0".into());
    record.state = ReleaseState::Completed;
    record.transitions.push(StateTransition::new(
        Some(ReleaseState::Verifying),
        ReleaseState::Completed,
        options.completed_at,
        None,
    ));
    record
}

#[test]
fn only_committed_wordpress_org_version_releases_are_tracked() {
    assert!(record(Options::default()).is_directory_tracked());
    assert!(
        record(Options {
            url: "https://themes.svn.wordpress.org/acme-theme/",
            ..Options::default()
        })
        .is_directory_tracked()
    );
    assert!(
        !record(Options {
            url: "file:///tmp/repo/acme-gallery",
            ..Options::default()
        })
        .is_directory_tracked()
    );
    assert!(
        !record(Options {
            mode: ReleaseMode::DryRun,
            ..Options::default()
        })
        .is_directory_tracked()
    );
    assert!(
        !record(Options {
            kind: ReleaseKind::ReadmeUpdate,
            ..Options::default()
        })
        .is_directory_tracked()
    );
    let mut failed = record(Options::default());
    failed.state = ReleaseState::Failed;
    assert!(!failed.is_directory_tracked());
    // Rust-only: the host comparison ignores case, like Swift's `.lowercased()`.
    assert!(
        record(Options {
            url: "https://Plugins.SVN.WordPress.org/acme-gallery/",
            ..Options::default()
        })
        .is_directory_tracked()
    );
}

#[test]
fn awaiting_window_and_final_states() {
    let now = SwiftDate::now();
    assert!(
        record(Options {
            completed_at: now.adding(-3600.0),
            ..Options::default()
        })
        .awaits_directory(now, DIRECTORY_WAIT_WINDOW)
    );
    assert!(
        !record(Options {
            completed_at: now.adding(-73.0 * 3600.0),
            ..Options::default()
        })
        .awaits_directory(now, DIRECTORY_WAIT_WINDOW),
        "gives up after 72 hours"
    );
    let mut live = record(Options::default());
    live.directory_status = Some(DirectoryStatus::new(
        DirectoryState::Live,
        Some("2.4.0".into()),
        SwiftDate::now(),
        None,
    ));
    assert!(!live.awaits_directory(now, DIRECTORY_WAIT_WINDOW));
    let mut pending = record(Options::default());
    pending.directory_status = Some(DirectoryStatus::new(
        DirectoryState::Pending,
        Some("2.3.0".into()),
        SwiftDate::now(),
        None,
    ));
    assert!(pending.awaits_directory(now, DIRECTORY_WAIT_WINDOW));
}
