//! Ported from `WordPressOrgTests.swift` (`WordPressOrgTests`), `DirectoryStatusTests.swift`
//! (`testParsesDirectoryListing`) and `LiveRepositoryTests.swift`
//! (`LiveWordPressOrgAPITests.testRealEndpoints`, ignored). The HTTP tests against a local
//! server are Rust-only: the Swift service had no request-level tests.

mod common;

use common::http::{MockResponse, MockServer};
use rd_integrations::{
    DirectoryListing, WordPressBranch, WordPressOrgError, WordPressOrgService, WordPressOrgServicing,
};
use rd_model::ProductType;

// MARK: WordPressOrgTests

#[test]
fn parses_latest_version_from_version_check() {
    let json = r#"{"offers":[{"response":"upgrade","current":"7.1.5","version":"7.1.5"},{"response":"autoupdate","current":"7.0.9"}],"translations":[]}"#;
    assert_eq!(
        WordPressOrgService::parse_latest_version(json.as_bytes()).unwrap(),
        "7.1.5"
    );
}

#[test]
fn rejects_unexpected_version_check_payloads() {
    assert!(WordPressOrgService::parse_latest_version(br#"{"offers":[]}"#).is_err());
    assert!(WordPressOrgService::parse_latest_version(br#"{"offers":[{"current":"latest"}]}"#).is_err());
    assert!(WordPressOrgService::parse_latest_version(b"<html>").is_err());
}

#[test]
fn branches() {
    assert_eq!(WordPressBranch::of("7.1.5").as_deref(), Some("7.1"));
    assert_eq!(WordPressBranch::of("7.1").as_deref(), Some("7.1"));
    assert_eq!(WordPressBranch::of("7").as_deref(), Some("7.0"));
    assert_eq!(WordPressBranch::of("WP 6.9"), None);
    assert_eq!(WordPressBranch::compare("6.9", "7.1.5"), Some(-1));
    assert_eq!(WordPressBranch::compare("7.1", "7.1.5"), Some(0));
    assert_eq!(
        WordPressBranch::compare("7.1.2", "7.1.5"),
        Some(0),
        "point releases share a branch"
    );
    assert_eq!(WordPressBranch::compare("7.2", "7.1.5"), Some(1));
    assert_eq!(WordPressBranch::compare("soon", "7.1"), None);
}

// MARK: DirectoryStatusTests

#[test]
fn parses_directory_listing() {
    let json = r#"{"slug":"akismet","version":"5.7.2","tested":"7.1.5","last_updated":"2026-08-18 11:42pm GMT"}"#;
    let listing = WordPressOrgService::parse_listing(json.as_bytes()).unwrap();
    assert_eq!(
        listing,
        DirectoryListing {
            version: Some("5.7.2".into()),
            tested: Some("7.1.5".into()),
            last_updated: Some("2026-08-18 11:42pm GMT".into()),
        }
    );
    assert!(WordPressOrgService::parse_listing(br#"{"error":"Plugin not found."}"#).is_err());
}

// MARK: Requests (Rust-only)

async fn setup() -> (MockServer, WordPressOrgService) {
    let server = MockServer::start().await;
    let service = WordPressOrgService::with_base_url(server.base_url.clone()).expect("client");
    (server, service)
}

#[tokio::test]
async fn latest_version_request() {
    let (server, service) = setup().await;
    server.respond(|_| MockResponse::new(200).body(r#"{"offers":[{"current":"7.1.5"}]}"#));
    assert_eq!(service.latest_word_press_version().await.unwrap(), "7.1.5");
    let request = server.last_request().unwrap();
    assert_eq!(request.method, "GET");
    assert_eq!(request.path(), "/core/version-check/1.7/");
    assert_eq!(request.header("User-Agent"), Some("ReleaseDock"));

    server.respond(|_| MockResponse::new(503));
    assert_eq!(
        service.latest_word_press_version().await.unwrap_err(),
        WordPressOrgError::Http(503)
    );
    assert_eq!(
        WordPressOrgError::Http(503).to_string(),
        "WordPress.org returned HTTP 503."
    );
}

#[tokio::test]
async fn directory_listing_requests() {
    let (server, service) = setup().await;
    server.respond(|_| MockResponse::new(200).body(r#"{"version":"5.7.2"}"#));
    let listing = service.directory_listing("Akismet", ProductType::Plugin).await.unwrap();
    assert_eq!(listing, Some(DirectoryListing::new(Some("5.7.2".into()))));
    let request = server.last_request().unwrap();
    assert_eq!(request.path(), "/plugins/info/1.2/");
    assert_eq!(
        request.query(),
        Some(
            "action=plugin_information&request%5Bslug%5D=akismet&request%5Bfields%5D%5Bsections%5D=0&request%5Bfields%5D%5Bversions%5D=0"
        )
    );

    service
        .directory_listing("twenty twenty", ProductType::Theme)
        .await
        .unwrap();
    let request = server.last_request().unwrap();
    assert_eq!(request.path(), "/themes/info/1.2/");
    assert!(
        request
            .query()
            .unwrap()
            .starts_with("action=theme_information&request%5Bslug%5D=twenty%20twenty&")
    );

    server.respond(|_| MockResponse::new(404).body(r#"{"error":"Plugin not found."}"#));
    assert_eq!(
        service.directory_listing("missing", ProductType::Plugin).await.unwrap(),
        None
    );
    server.respond(|_| MockResponse::new(500));
    assert_eq!(
        service.directory_listing("x", ProductType::Plugin).await.unwrap_err(),
        WordPressOrgError::Http(500)
    );
}

// MARK: LiveWordPressOrgAPITests

/// Opt-in, read-only WordPress.org API checks:
/// `cargo test -p rd-integrations --test wordpress_org -- --ignored real_endpoints`
#[tokio::test]
#[ignore = "calls api.wordpress.org"]
async fn real_endpoints() {
    let service = WordPressOrgService::new().unwrap();
    let latest = service.latest_word_press_version().await.unwrap();
    assert!(WordPressBranch::of(&latest).is_some());
    let plugin = service.directory_listing("akismet", ProductType::Plugin).await.unwrap();
    assert!(plugin.as_ref().and_then(|p| p.version.as_ref()).is_some());
    let theme = service
        .directory_listing("twentytwentyfive", ProductType::Theme)
        .await
        .unwrap();
    assert!(theme.as_ref().and_then(|t| t.version.as_ref()).is_some());
    let missing = service
        .directory_listing("releasedock-no-such-plugin-zz9", ProductType::Plugin)
        .await
        .unwrap();
    assert!(missing.is_none());
    println!(
        "LIVE API: WordPress {latest}, akismet {:?}, twentytwentyfive {:?}",
        plugin.and_then(|p| p.version),
        theme.and_then(|t| t.version)
    );
}
