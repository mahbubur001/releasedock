//! `GitHubReleasePipelineTests` from `GitHubReleaseTests.swift` (the API client and the
//! publisher are tested in rd-integrations).
#![allow(clippy::unwrap_used)]

mod common;

use common::{Harness, LocalSvnRepository, StubGit, StubGitHubReleaser, TempDir, fixtures, product};
use rd_integrations::GitHubReleasing;
use rd_keychain::{InMemoryKeychain, Keychain, KeychainItem};
use rd_model::{GitHubReleaseSettings, ProductType};
use rd_pipeline::ReleasePipelineService;
use rd_records::{ApprovalKind, GitRepositoryState, ReleaseKind, ReleaseSource, ReleaseState};
use rd_store::{AppPaths, ProductRepository, ReleaseRepository, Store};
use rd_svn::SvnService;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

#[tokio::test]
async fn completed_release_creates_git_hub_release_with_the_exact_package() {
    let Some(repo) = LocalSvnRepository::new_or_skip() else {
        return;
    };
    let work = TempDir::new();
    let store = Arc::new(Store::in_memory().unwrap());
    let paths = AppPaths::new(work.path("data"));
    paths.create_all().unwrap();
    let keychain = Arc::new(InMemoryKeychain::new());
    keychain
        .set_password("t0k", &KeychainItem::github_release_token())
        .unwrap();
    let stub = Arc::new(StubGitHubReleaser::default());
    let git = StubGit::with_state(GitRepositoryState {
        is_repository: true,
        head_commit: Some("abc1234567".into()),
        branch: Some("main".into()),
        exact_tag: None,
        uncommitted_changes: Vec::new(),
        remote_url: None,
    });
    let svn = Arc::new(SvnService::new().with_command_timeout(Duration::from_secs(60)));
    let mut deps = Harness::dependencies(&store, svn, &paths, Arc::new(git))
        .with_github_releaser(Some(stub.clone() as Arc<dyn GitHubReleasing>));
    deps.keychain = keychain;
    let pipeline = ReleasePipelineService::new(deps);
    repo.create_plugin("sample-plugin", Some("1.0.0"), "Sample Plugin")
        .unwrap();
    let mut product = product(
        "Sample Plugin",
        "sample-plugin",
        ProductType::Plugin,
        &format!("{}/sample-plugin", repo.url()),
        &work.path("wc-plugin"),
        Some("1.0.0"),
    );
    product.github_repository_url = Some("https://github.com/acme/sample-plugin".into());
    product.github_release = Some(GitHubReleaseSettings {
        enabled: true,
        ..GitHubReleaseSettings::default()
    });
    store.save_product(&product).unwrap();
    let source = fixtures::Plugin::new("1.1.0").at("dev/sample-plugin").make(&work);

    let mut record = pipeline
        .create_release(
            product.id,
            ReleaseSource::folder(source.to_string_lossy()),
            None,
            ReleaseKind::Version,
            None,
        )
        .unwrap();
    pipeline.prepare(record.id).await.unwrap();
    pipeline.approve(record.id, ApprovalKind::User).await.unwrap();
    record = pipeline.publish(record.id).await.unwrap();
    assert_eq!(record.state, ReleaseState::Completed, "{:?}", record.errors);
    assert_eq!(
        record.github_release.as_ref().map(|g| g.succeeded()),
        Some(true),
        "{:?}",
        record.github_release
    );
    assert_eq!(
        stub.created.lock().unwrap().first().map(|c| c.tag.clone()).as_deref(),
        Some("v1.1.0")
    );
    let (name, entries) = stub.uploads.lock().unwrap().first().cloned().unwrap();
    assert_eq!(name, "sample-plugin-1.1.0.zip");
    assert!(
        entries.iter().any(|e| e == "sample-plugin/sample-plugin.php"),
        "{entries:?}"
    );
    assert!(entries.iter().all(|e| e.starts_with("sample-plugin/")));
    assert!(Path::new(record.release_zip_path.as_ref().unwrap()).exists());
    assert_eq!(
        store
            .release(record.id)
            .unwrap()
            .unwrap()
            .github_release
            .map(|g| g.tag)
            .as_deref(),
        Some("v1.1.0"),
        "persisted"
    );
}
