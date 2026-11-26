//! The pipeline tests of `DirectoryStatusTests.swift` (`refreshDirectoryStatus`). The pure
//! tests are in rd-records (`tests/directory_status.rs`) and the listing parser in
//! rd-integrations (`tests/wordpress_org.rs`). These need no svn.
#![allow(clippy::unwrap_used)]

mod common;

use common::{RecordingNotifications, StubGit, StubWordPressOrg, TempDir};
use rd_integrations::WordPressOrgServicing;
use rd_keychain::InMemoryKeychain;
use rd_model::{Id, Product, ProductType, ReleaseMode, SwiftDate};
use rd_pipeline::{PipelineDependencies, ReleasePipelineService, ReleaseStagingService};
use rd_records::{
    DirectoryListing, DirectoryState, DirectoryStatus, ReleaseKind, ReleaseRecord, ReleaseSource, ReleaseState,
    StateTransition,
};
use rd_store::{AppPaths, ProductRepository, ReleaseRepository, Store};
use rd_svn::SvnService;
use std::sync::Arc;

fn acme() -> Product {
    Product::new(
        "Acme Gallery",
        "acme-gallery",
        ProductType::Plugin,
        "https://plugins.svn.wordpress.org/acme-gallery/",
        "/tmp/wc-acme",
    )
}

fn record_with(
    product: &Product,
    url: &str,
    mode: ReleaseMode,
    kind: ReleaseKind,
    completed_at: SwiftDate,
) -> ReleaseRecord {
    let mut product = product.clone();
    product.svn_repository_url = url.into();
    let mut record = ReleaseRecord::new_with(
        Id::new(),
        &product,
        ReleaseSource::zip("/tmp/acme.zip"),
        mode,
        kind,
        completed_at,
    );
    record.version = Some("2.4.0".into());
    record.state = ReleaseState::Completed;
    record.transitions.push(StateTransition::new(
        Some(ReleaseState::Verifying),
        ReleaseState::Completed,
        completed_at,
        None,
    ));
    record
}

fn record(product: &Product) -> ReleaseRecord {
    record_with(
        product,
        "https://plugins.svn.wordpress.org/acme-gallery/",
        ReleaseMode::Assisted,
        ReleaseKind::Version,
        SwiftDate::now(),
    )
}

struct Setup {
    pipeline: ReleasePipelineService,
    store: Arc<Store>,
    notifications: Arc<RecordingNotifications>,
    product: Product,
    _work: TempDir,
}

fn make_pipeline(wordpress_org: Option<Arc<StubWordPressOrg>>) -> Setup {
    let work = TempDir::new();
    let store = Arc::new(Store::in_memory().unwrap());
    let product = acme();
    store.save_product(&product).unwrap();
    let paths = AppPaths::new(work.path("data"));
    paths.create_all().unwrap();
    let notifications = Arc::new(RecordingNotifications::default());
    let deps = PipelineDependencies::new(
        store.clone(),
        store.clone(),
        Arc::new(SvnService::new()),
        Arc::new(StubGit::new()),
        Arc::new(InMemoryKeychain::new()),
        Arc::new(ReleaseStagingService::new(paths)),
    )
    .with_notifications(notifications.clone())
    .with_wordpress_org(wordpress_org.map(|w| w as Arc<dyn WordPressOrgServicing>))
    .with_generator("tests");
    Setup {
        pipeline: ReleasePipelineService::new(deps),
        store,
        notifications,
        product,
        _work: work,
    }
}

#[tokio::test]
async fn pending_then_live_notifies_once() {
    let wordpress_org = Arc::new(StubWordPressOrg::new(Some(DirectoryListing::new(Some("2.3.0".into())))));
    let s = make_pipeline(Some(wordpress_org.clone()));
    let release = record(&s.product);
    s.store.save_release(&release).unwrap();

    let mut updated = s.pipeline.refresh_directory_status(release.id).await.unwrap();
    let status = updated.directory_status.clone().unwrap();
    assert_eq!(status.state, DirectoryState::Pending);
    assert_eq!(status.listed_version.as_deref(), Some("2.3.0"));
    assert_eq!(status.live_since, None);
    assert_eq!(
        updated.state,
        ReleaseState::Completed,
        "the release state never changes"
    );

    wordpress_org.set_listing(Some(DirectoryListing::new(Some("2.4.0".into()))));
    updated = s.pipeline.refresh_directory_status(release.id).await.unwrap();
    assert_eq!(
        updated.directory_status.as_ref().map(|d| d.state),
        Some(DirectoryState::Live)
    );
    let live_since = updated.directory_status.as_ref().unwrap().live_since.unwrap();
    updated = s.pipeline.refresh_directory_status(release.id).await.unwrap();
    assert_eq!(
        updated.directory_status.as_ref().unwrap().live_since,
        Some(live_since),
        "first time live is kept"
    );
    let live: Vec<String> = s
        .notifications
        .titles()
        .into_iter()
        .filter(|t| t.contains("is live"))
        .collect();
    assert_eq!(live, ["Acme Gallery 2.4.0 is live"]);
    assert_eq!(
        s.store
            .release(release.id)
            .unwrap()
            .unwrap()
            .directory_status
            .map(|d| d.state),
        Some(DirectoryState::Live),
        "persisted"
    );
}

#[tokio::test]
async fn refresh_awaiting_skips_untracked_and_finished_releases() {
    let wordpress_org = Arc::new(StubWordPressOrg::new(Some(DirectoryListing::new(Some("2.3.0".into())))));
    let s = make_pipeline(Some(wordpress_org.clone()));
    let awaiting = record(&s.product);
    let mut live = record(&s.product);
    live.directory_status = Some(DirectoryStatus::new(
        DirectoryState::Live,
        Some("2.4.0".into()),
        SwiftDate::now(),
        None,
    ));
    let local = record_with(
        &s.product,
        "file:///tmp/repo/acme-gallery",
        ReleaseMode::Assisted,
        ReleaseKind::Version,
        SwiftDate::now(),
    );
    let old = record_with(
        &s.product,
        "https://plugins.svn.wordpress.org/acme-gallery/",
        ReleaseMode::Assisted,
        ReleaseKind::Version,
        SwiftDate::now().adding(-80.0 * 3600.0),
    );
    for r in [&awaiting, &live, &local, &old] {
        s.store.save_release(r).unwrap();
    }

    let refreshed = s.pipeline.refresh_awaiting_directory_releases(SwiftDate::now()).await;
    let ids: Vec<Id> = refreshed.iter().map(|r| r.id).collect();
    assert_eq!(ids, [awaiting.id]);
    assert_eq!(wordpress_org.requests(), 1);
}

#[tokio::test]
async fn without_word_press_org_service_nothing_happens() {
    let s = make_pipeline(None);
    let release = record(&s.product);
    s.store.save_release(&release).unwrap();
    let updated = s.pipeline.refresh_directory_status(release.id).await.unwrap();
    assert_eq!(updated.directory_status, None);
    let refreshed = s.pipeline.refresh_awaiting_directory_releases(SwiftDate::now()).await;
    assert!(refreshed.is_empty());
}

#[tokio::test]
async fn not_listed_product() {
    let s = make_pipeline(Some(Arc::new(StubWordPressOrg::new(None))));
    let release = record(&s.product);
    s.store.save_release(&release).unwrap();
    let updated = s.pipeline.refresh_directory_status(release.id).await.unwrap();
    assert_eq!(
        updated.directory_status.map(|d| d.state),
        Some(DirectoryState::NotListed)
    );
    assert!(s.notifications.titles().is_empty());
}
