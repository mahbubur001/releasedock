//! Ported from `GitHubReleaseTests.swift`: `GitHubReleaseAPITests` and
//! `GitHubReleasePublisherTests` (the pipeline test is listed in the README).

mod common;

use async_trait::async_trait;
use common::TempDir;
use common::http::{MockResponse, MockServer};
use rd_integrations::{
    CreateRelease, GitHubError, GitHubRelease, GitHubReleasePublisher, GitHubReleasing, GitHubRepository, GitHubService,
};
use rd_model::{Product, ProductType, ReleaseMode};
use rd_records::{GitHubReleaseSettings, ReleaseRecord, ReleaseSource};
use rd_versioning::{DetectedProduct, readme};
use std::path::Path;
use std::sync::{Arc, Mutex};

const RELEASE_JSON: &str = r#"{"id": 7, "tag_name": "v2.4.0", "name": "2.4.0", "body": "x", "draft": false, "prerelease": false,
 "html_url": "https://github.com/acme/acme-gallery/releases/tag/v2.4.0", "published_at": null, "assets": [],
 "upload_url": "https://uploads.github.com/repos/acme/acme-gallery/releases/7/assets{?name,label}"}"#;

fn repo() -> GitHubRepository {
    GitHubRepository::new("acme", "acme-gallery")
}

async fn setup() -> (MockServer, GitHubService) {
    let server = MockServer::start().await;
    let service = GitHubService::with_base_url(server.base_url.clone()).expect("client");
    (server, service)
}

fn new_release<'a>(tag: &'a str, commit: &'a str, name: &'a str, body: &'a str) -> CreateRelease<'a> {
    CreateRelease {
        tag,
        commit,
        name,
        body,
        draft: false,
        prerelease: false,
    }
}

// MARK: GitHubReleaseAPITests

#[tokio::test]
async fn create_release_targets_the_commit() {
    let (server, service) = setup().await;
    server.respond(|_| MockResponse::new(201).body(RELEASE_JSON));
    let release = service
        .create_release(&repo(), &new_release("v2.4.0", "abc123", "2.4.0", "Notes"), "t0k")
        .await
        .unwrap();
    assert_eq!(
        release.upload_url.as_deref(),
        Some("https://uploads.github.com/repos/acme/acme-gallery/releases/7/assets{?name,label}")
    );
    let request = server.last_request().unwrap();
    assert_eq!(request.method, "POST");
    assert_eq!(request.path(), "/repos/acme/acme-gallery/releases");
    assert_eq!(request.header("Authorization"), Some("Bearer t0k"));
    let body = request.json();
    assert_eq!(body["target_commitish"], "abc123");
    assert_eq!(body["tag_name"], "v2.4.0");
}

#[tokio::test]
async fn errors_are_specific() {
    let (server, service) = setup().await;
    server.respond(|_| {
        MockResponse::new(422).body(
            r#"{"message":"Validation Failed","errors":[{"resource":"Release","code":"already_exists","field":"tag_name"}]}"#,
        )
    });
    let error = service
        .create_release(&repo(), &new_release("v1", "a", "1", ""), "t")
        .await
        .unwrap_err();
    assert_eq!(
        error,
        GitHubError::ValidationFailed("Validation Failed: already_exists".into())
    );

    server.respond(|_| MockResponse::new(403));
    let error = service.release("v1", &repo(), "t").await.unwrap_err();
    assert_eq!(error, GitHubError::Forbidden);

    server.respond(|_| MockResponse::new(404));
    let missing = service.release("v1", &repo(), "t").await.unwrap();
    assert!(missing.is_none());

    server.respond(|_| MockResponse::new(422));
    let exists = service.commit_exists("abc", &repo(), "t").await.unwrap();
    assert!(!exists);
}

#[tokio::test]
async fn upload_expands_the_template() {
    let release = GitHubRelease::decode(RELEASE_JSON.as_bytes()).unwrap();
    assert_eq!(
        GitHubService::upload_url(release.upload_url.as_deref().unwrap(), "acme-gallery-2.4.0.zip")
            .unwrap()
            .as_str(),
        "https://uploads.github.com/repos/acme/acme-gallery/releases/7/assets?name=acme-gallery-2.4.0.zip"
    );

    // The request itself goes to the local server instead of uploads.github.com.
    let (server, service) = setup().await;
    let mut local = release.clone();
    local.upload_url = Some(format!(
        "{}repos/acme/acme-gallery/releases/7/assets{{?name,label}}",
        server.base_url
    ));
    let dir = TempDir::new();
    let zip = dir.write("a.zip", "PK");
    server.respond(|_| MockResponse::new(201).body("{}"));
    service
        .upload_asset(&zip, "acme-gallery-2.4.0.zip", &local, "t")
        .await
        .unwrap();
    let request = server.last_request().unwrap();
    assert_eq!(
        request.target,
        "/repos/acme/acme-gallery/releases/7/assets?name=acme-gallery-2.4.0.zip"
    );
    assert_eq!(request.header("Content-Type"), Some("application/zip"));
    assert_eq!(request.body, b"PK");
}

// MARK: StubGitHubReleaser

#[derive(Clone, Debug)]
struct Created {
    tag: String,
    commit: String,
    body: String,
    prerelease: bool,
}

/// Records calls and plays back configured answers.
struct StubGitHubReleaser {
    existing: Mutex<Option<GitHubRelease>>,
    commit_on_github: Mutex<bool>,
    created: Mutex<Vec<Created>>,
    /// Uploaded asset names. (Swift also lists the ZIP's entries for the pipeline test.)
    uploads: Mutex<Vec<String>>,
}

impl StubGitHubReleaser {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            existing: Mutex::new(None),
            commit_on_github: Mutex::new(true),
            created: Mutex::new(Vec::new()),
            uploads: Mutex::new(Vec::new()),
        })
    }

    fn release(tag: &str, assets: &[&str]) -> GitHubRelease {
        let asset_json = assets
            .iter()
            .enumerate()
            .map(|(i, name)| {
                format!(
                    r#"{{"id": {i}, "name": "{name}", "size": 1, "url": "https://api.github.com/a/{i}", "browser_download_url": "https://github.com/d/{name}"}}"#
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let json = format!(
            r#"{{"id": 7, "tag_name": "{tag}", "name": null, "body": null, "draft": false, "prerelease": false,
 "html_url": "https://github.com/acme/acme-gallery/releases/tag/{tag}", "published_at": null, "assets": [{asset_json}],
 "upload_url": "https://uploads.github.com/repos/acme/acme-gallery/releases/7/assets{{?name,label}}"}}"#
        );
        GitHubRelease::decode(json.as_bytes()).expect("stub release")
    }

    fn created(&self) -> Vec<Created> {
        self.created.lock().expect("lock").clone()
    }

    fn uploads(&self) -> Vec<String> {
        self.uploads.lock().expect("lock").clone()
    }
}

#[async_trait]
impl GitHubReleasing for StubGitHubReleaser {
    async fn commit_exists(&self, _: &str, _: &GitHubRepository, _: &str) -> Result<bool, GitHubError> {
        Ok(*self.commit_on_github.lock().expect("lock"))
    }

    async fn release(&self, _: &str, _: &GitHubRepository, _: &str) -> Result<Option<GitHubRelease>, GitHubError> {
        Ok(self.existing.lock().expect("lock").clone())
    }

    async fn create_release(
        &self,
        _: &GitHubRepository,
        release: &CreateRelease<'_>,
        _: &str,
    ) -> Result<GitHubRelease, GitHubError> {
        self.created.lock().expect("lock").push(Created {
            tag: release.tag.into(),
            commit: release.commit.into(),
            body: release.body.into(),
            prerelease: release.prerelease,
        });
        Ok(Self::release(release.tag, &[]))
    }

    async fn upload_asset(&self, _: &Path, name: &str, _: &GitHubRelease, _: &str) -> Result<(), GitHubError> {
        self.uploads.lock().expect("lock").push(name.into());
        Ok(())
    }
}

// MARK: GitHubReleasePublisherTests

fn record(commit: Option<&str>, version: &str) -> (ReleaseRecord, Product) {
    let mut product = Product::new(
        "Acme Gallery",
        "acme-gallery",
        ProductType::Plugin,
        "https://plugins.svn.wordpress.org/acme-gallery/",
        "/tmp/wc",
    );
    product.github_repository_url = Some("https://github.com/acme/acme-gallery".into());
    product.github_release = Some(GitHubReleaseSettings {
        enabled: true,
        ..Default::default()
    });
    let mut record = ReleaseRecord::new(&product, ReleaseSource::folder("/tmp/src"), ReleaseMode::Assisted);
    record.version = Some(version.into());
    record.git_commit = commit.map(str::to_owned);
    record.committed_revision = Some(42);
    let mut detected = DetectedProduct::new(ProductType::Plugin);
    detected.version = Some(version.into());
    detected.readme = Some(readme::parse(&format!(
        "=== A ===\nStable tag: {version}\n\nShort.\n\n== Changelog ==\n= {version} =\n* New: Lightbox.\n"
    )));
    record.detected = Some(detected);
    (record, product)
}

fn default_record() -> (ReleaseRecord, Product) {
    record(Some("abc1234567"), "2.4.0")
}

#[tokio::test]
async fn creates_release_on_the_released_commit() {
    let stub = StubGitHubReleaser::new();
    let (record, product) = default_record();
    let result = GitHubReleasePublisher::new(stub.clone())
        .publish(&record, &product, None, Some("t"))
        .await;
    assert!(result.succeeded(), "{result:?}");
    assert_eq!(result.tag, "v2.4.0");
    let created = stub.created();
    assert_eq!(created.first().map(|c| c.tag.as_str()), Some("v2.4.0"));
    assert_eq!(created.first().map(|c| c.commit.as_str()), Some("abc1234567"));
    assert_eq!(
        created.first().map(|c| c.body.as_str()),
        Some("* New: Lightbox.\n\nPublished to WordPress.org in SVN revision r42 with ReleaseDock.")
    );
    assert_eq!(created.first().map(|c| c.prerelease), Some(false));
    assert!(result.notes.iter().any(|n| n.contains("ZIP was no longer available")));
}

#[tokio::test]
async fn prerelease_flag() {
    let stub = StubGitHubReleaser::new();
    let (record, product) = record(Some("abc1234567"), "2.5.0-beta1");
    GitHubReleasePublisher::new(stub.clone())
        .publish(&record, &product, None, Some("t"))
        .await;
    assert_eq!(stub.created().first().map(|c| c.prerelease), Some(true));
}

#[tokio::test]
async fn reuses_an_existing_release_and_adds_the_missing_asset() {
    let stub = StubGitHubReleaser::new();
    *stub.existing.lock().unwrap() = Some(StubGitHubReleaser::release("v2.4.0", &[]));
    let dir = TempDir::new();
    let zip = dir.write("x.zip", "PK");
    let (record, product) = default_record();
    let publisher = GitHubReleasePublisher::new(stub.clone());
    let result = publisher.publish(&record, &product, Some(&zip), Some("t")).await;
    assert!(result.succeeded());
    assert!(result.already_existed);
    assert!(stub.created().is_empty(), "never a second release for the tag");
    assert_eq!(stub.uploads(), vec!["acme-gallery-2.4.0.zip".to_owned()]);

    *stub.existing.lock().unwrap() = Some(StubGitHubReleaser::release("v2.4.0", &["acme-gallery-2.4.0.zip"]));
    publisher.publish(&record, &product, Some(&zip), Some("t")).await;
    assert_eq!(stub.uploads().len(), 1, "an existing asset is not uploaded again");
}

#[tokio::test]
async fn explains_what_is_missing() {
    let stub = StubGitHubReleaser::new();
    let publisher = GitHubReleasePublisher::new(stub.clone());
    let (mut record, mut product) = default_record();
    let result = publisher.publish(&record, &product, None, None).await;
    assert!(result.error.as_deref().is_some_and(|e| e.contains("token")));
    let result = publisher
        .publish(&self::record(None, "2.4.0").0, &product, None, Some("t"))
        .await;
    assert!(result.error.as_deref().is_some_and(|e| e.contains("not a ZIP")));
    *stub.commit_on_github.lock().unwrap() = false;
    let result = publisher.publish(&record, &product, None, Some("t")).await;
    assert_eq!(
        result.error,
        Some(GitHubError::CommitNotOnGitHub("abc1234567".into()).to_string())
    );
    product.github_repository_url = None;
    record.version = Some("2.4.0".into());
    let result = publisher.publish(&record, &product, None, Some("t")).await;
    assert!(result.error.as_deref().is_some_and(|e| e.contains("GitHub repository")));
    assert!(stub.created().is_empty());
}
