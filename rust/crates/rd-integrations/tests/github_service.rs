//! Ported from `GitHubServiceTests.swift`. `StubURLProtocol` becomes a local HTTP server.

mod common;

use common::http::{MockResponse, MockServer};
use rd_integrations::{GitHubError, GitHubRepository, GitHubService, GitHubServicing, ReleaseChannel};
use rd_model::SwiftDate;

fn repo() -> GitHubRepository {
    GitHubRepository::new("acme", "releasedock")
}

async fn setup() -> (MockServer, GitHubService) {
    let server = MockServer::start().await;
    let service = GitHubService::with_base_url(server.base_url.clone()).expect("client");
    (server, service)
}

fn release_json(tag: &str, prerelease: bool, draft: bool) -> String {
    format!(
        r#"{{"id": 1, "tag_name": "{tag}", "name": "ReleaseDock {tag}", "body": "Notes", "draft": {draft}, "prerelease": {prerelease},
 "html_url": "https://github.com/acme/releasedock/releases/tag/{tag}", "published_at": "2026-11-01T10:00:00Z",
 "assets": [{{"id": 9, "name": "ReleaseDock.zip", "size": 10, "url": "https://api.github.com/repos/acme/releasedock/releases/assets/9",
             "browser_download_url": "https://github.com/acme/releasedock/releases/download/{tag}/ReleaseDock.zip", "content_type": "application/zip"}}]}}"#
    )
}

#[tokio::test]
async fn latest_stable_release_and_headers() {
    let (server, service) = setup().await;
    server.respond(|_| {
        MockResponse::new(200)
            .header("ETag", "\"abc\"")
            .header("x-ratelimit-remaining", "59")
            .header("x-ratelimit-limit", "60")
            .body(release_json("v1.4.0", false, false))
    });
    let result = service
        .latest_release(&repo(), ReleaseChannel::Stable, None, None)
        .await
        .unwrap();
    assert_eq!(result.release.as_ref().map(|r| r.version()).as_deref(), Some("1.4.0"));
    assert_eq!(result.etag.as_deref(), Some("\"abc\""));
    assert_eq!(result.rate_limit.and_then(|r| r.remaining), Some(59));
    let request = server.last_request().unwrap();
    assert_eq!(request.path(), "/repos/acme/releasedock/releases/latest");
    assert_eq!(request.header("Accept"), Some("application/vnd.github+json"));
    assert!(request.header("X-GitHub-Api-Version").is_some());
    assert!(
        request.header("Authorization").is_none(),
        "public checks are unauthenticated"
    );
    // Rust-only: the ISO 8601 date is decoded.
    assert_eq!(
        result.release.unwrap().published_at,
        rd_model::iso8601::parse("2026-11-01T10:00:00Z")
    );
}

#[tokio::test]
async fn conditional_request_uses_cache() {
    let (server, service) = setup().await;
    server.respond(|_| {
        MockResponse::new(200)
            .header("ETag", "\"abc\"")
            .body(release_json("1.4.0", false, false))
    });
    let first = service
        .latest_release(&repo(), ReleaseChannel::Stable, None, None)
        .await
        .unwrap();
    server.respond(|_| MockResponse::new(304));
    let second = service
        .latest_release(&repo(), ReleaseChannel::Stable, Some("github_pat_x"), Some(&first))
        .await
        .unwrap();
    let request = server.last_request().unwrap();
    assert_eq!(request.header("If-None-Match"), Some("\"abc\""));
    assert_eq!(second.release.map(|r| r.version()).as_deref(), Some("1.4.0"));
    assert_eq!(request.header("Authorization"), Some("Bearer github_pat_x"));
}

#[tokio::test]
async fn beta_channel_picks_highest_non_draft() {
    let (server, service) = setup().await;
    let body = format!(
        "[{}, {}, {}]",
        release_json("1.5.0-beta1", true, false),
        release_json("1.4.0", false, false),
        release_json("2.0.0", false, true)
    );
    server.respond(move |_| MockResponse::new(200).body(body.clone()));
    let result = service
        .latest_release(&repo(), ReleaseChannel::Beta, None, None)
        .await
        .unwrap();
    assert_eq!(result.release.map(|r| r.version()).as_deref(), Some("1.5.0-beta1"));
    assert_eq!(server.last_request().unwrap().query(), Some("per_page=20"));
}

#[tokio::test]
async fn rate_limit_and_errors() {
    let (server, service) = setup().await;
    server.respond(|_| {
        MockResponse::new(403)
            .header("x-ratelimit-remaining", "0")
            .header("x-ratelimit-reset", "1900000000")
    });
    let error = service
        .latest_release(&repo(), ReleaseChannel::Stable, None, None)
        .await
        .unwrap_err();
    assert_eq!(
        error,
        GitHubError::RateLimited(Some(SwiftDate::from_unix_seconds(1_900_000_000.0)))
    );

    server.respond(|_| MockResponse::new(404));
    let error = service
        .latest_release(&repo(), ReleaseChannel::Stable, None, None)
        .await
        .unwrap_err();
    assert_eq!(error, GitHubError::NotFound);

    server.respond(|_| MockResponse::new(401));
    let error = service
        .latest_release(&repo(), ReleaseChannel::Stable, Some("bad"), None)
        .await
        .unwrap_err();
    assert_eq!(error, GitHubError::Unauthorized);
}

#[test]
fn repository_parsing() {
    let full = |s: &str| GitHubRepository::parse(s).map(|r| r.full_name());
    assert_eq!(
        full("https://github.com/acme/release-dock.git").as_deref(),
        Some("acme/release-dock")
    );
    assert_eq!(full("git@github.com:acme/x.git").as_deref(), Some("acme/x"));
    assert_eq!(full("acme/x").as_deref(), Some("acme/x"));
    assert_eq!(full("https://github.com/acme"), None);
    assert_eq!(full("acme/../x"), None);
    assert_eq!(full("acme/x?y=1"), None);
    // Rust-only: prefixes are matched case-insensitively, the web URL is derived.
    assert_eq!(full("HTTPS://GitHub.com/acme/x/").as_deref(), Some("acme/x"));
    assert_eq!(
        GitHubRepository::new("acme", "x").html_url(),
        "https://github.com/acme/x"
    );
}

#[test]
fn error_messages_match_swift() {
    assert_eq!(
        GitHubError::RateLimited(None).to_string(),
        "GitHub API rate limit reached."
    );
    assert!(
        GitHubError::RateLimited(Some(SwiftDate::from_unix_seconds(1_900_000_000.0)))
            .to_string()
            .starts_with("GitHub API rate limit reached. Try again after ")
    );
    assert_eq!(
        GitHubError::CommitNotOnGitHub("abc1234567890".into()).to_string(),
        "Commit abc1234567 is not on GitHub. Push it, then create the release again."
    );
    assert_eq!(GitHubError::Http(502).to_string(), "GitHub returned HTTP 502.");
}
