//! Ported from `RepositorySetupTests.swift` and `LiveRepositoryTests.swift`
//! (`testDescribeRealRepositoryAndCheckout`, ignored). Tests that need `svn`/`svnadmin`
//! skip themselves (print `skipped: …` and pass) when either is missing.

mod common;

use common::{LocalSvnRepository, TempDir, strings};
use rd_integrations::{RepositorySetupError, RepositorySetupService};
use rd_model::ProductType;
use rd_svn::SvnService;
use std::sync::Arc;

fn service() -> RepositorySetupService {
    RepositorySetupService::new(Arc::new(SvnService::new()))
}

#[test]
fn url_normalization() {
    let url = |input: &str, product_type| RepositorySetupService::repository_url(input, product_type);
    assert_eq!(
        url("my-plugin", ProductType::Plugin).unwrap(),
        "https://plugins.svn.wordpress.org/my-plugin"
    );
    assert_eq!(
        url("my-theme", ProductType::Theme).unwrap(),
        "https://themes.svn.wordpress.org/my-theme"
    );
    assert_eq!(
        url("https://plugins.svn.wordpress.org/x/", ProductType::Plugin).unwrap(),
        "https://plugins.svn.wordpress.org/x"
    );
    assert!(url("http://plugins.svn.wordpress.org/x", ProductType::Plugin).is_err());
    assert!(url("https://u:p@plugins.svn.wordpress.org/x", ProductType::Plugin).is_err());
    // Rust-only: a host without a path and the error message.
    assert!(url("https://plugins.svn.wordpress.org/", ProductType::Plugin).is_err());
    assert_eq!(
        url(" ftp://x/y ", ProductType::Plugin).unwrap_err().to_string(),
        "“ftp://x/y” is not a valid https SVN URL."
    );
}

#[tokio::test]
async fn checkout_from_server_detects_everything() {
    let Some(repo) = LocalSvnRepository::new_or_skip() else {
        return;
    };
    let work = TempDir::new();
    let service = service();
    repo.create_plugin("sample-plugin", Some("1.0.0"), "Sample Plugin")
        .unwrap();
    let url = format!("{}/sample-plugin", repo.url());
    let description = service.describe_remote(&url, None).await.unwrap();
    assert_eq!(description.product_type, Some(ProductType::Plugin));
    assert_eq!(description.slug, "sample-plugin");
    assert_eq!(description.current_version.as_deref(), Some("1.0.0"));
    assert_eq!(description.product_name.as_deref(), Some("Sample Plugin"));

    let destination = work.path("checkout");
    service
        .checkout(&description, ProductType::Plugin, &destination)
        .await
        .unwrap();
    assert!(destination.join("trunk/sample-plugin.php").exists());
    assert!(destination.join("tags/1.0.0").exists());
    assert!(
        !destination.join("tags/1.0.0/readme.txt").exists(),
        "tags are checked out sparsely"
    );

    let product = service.make_product(&description, ProductType::Plugin, &destination);
    assert_eq!(product.name, "Sample Plugin");
    assert_eq!(product.expected_header_name, "Sample Plugin");
    assert_eq!(product.current_version.as_deref(), Some("1.0.0"));
    assert_eq!(product.main_plugin_file, None, "slug.php is found automatically");
    assert_eq!(product.working_copy_path, destination.to_string_lossy());
}

#[tokio::test]
async fn checkout_refuses_non_empty_folder() {
    let Some(repo) = LocalSvnRepository::new_or_skip() else {
        return;
    };
    let work = TempDir::new();
    let service = service();
    repo.create_plugin("sample-plugin", Some("1.0.0"), "Sample Plugin")
        .unwrap();
    let description = service
        .describe_remote(&format!("{}/sample-plugin", repo.url()), None)
        .await
        .unwrap();
    work.write("busy/file.txt", "x");
    let error = service
        .checkout(&description, ProductType::Plugin, &work.path("busy"))
        .await
        .unwrap_err();
    assert_eq!(
        error,
        RepositorySetupError::DestinationNotEmpty(work.path("busy").to_string_lossy().into_owned())
    );
}

#[tokio::test]
async fn rejects_trunk_and_missing_repositories() {
    let Some(repo) = LocalSvnRepository::new_or_skip() else {
        return;
    };
    let service = service();
    repo.create_plugin("sample-plugin", Some("1.0.0"), "Sample Plugin")
        .unwrap();
    match service
        .describe_remote(&format!("{}/sample-plugin/trunk", repo.url()), None)
        .await
    {
        Err(RepositorySetupError::NotProductRoot { suggestion, .. }) => {
            assert!(suggestion.ends_with("/sample-plugin"), "{suggestion}");
        }
        other => panic!("trunk must be rejected: {other:?}"),
    }
    let missing = format!("{}/missing", repo.url());
    let error = service.describe_remote(&missing, None).await.unwrap_err();
    assert_eq!(error, RepositorySetupError::RepositoryNotFound(missing));
}

#[tokio::test]
async fn existing_working_copy_is_adopted() {
    let Some(repo) = LocalSvnRepository::new_or_skip() else {
        return;
    };
    let work = TempDir::new();
    let service = service();
    repo.create_theme("sample-theme", Some("1.2.0"), "Sample Theme")
        .unwrap();
    let checkout = work.path("existing");
    repo.run(&strings(&[
        "checkout",
        &format!("{}/sample-theme", repo.url()),
        &checkout.to_string_lossy(),
    ]))
    .unwrap();
    let description = service.describe_working_copy(&checkout).await.unwrap();
    assert_eq!(description.repository.product_type, Some(ProductType::Theme));
    assert_eq!(description.repository.current_version.as_deref(), Some("1.2.0"));
    assert_eq!(description.repository.product_name.as_deref(), Some("Sample Theme"));
    assert_eq!(description.local_changes, Vec::<String>::new());

    let product = service.make_product(&description.repository, ProductType::Theme, &checkout);
    assert_eq!(product.slug, "sample-theme");
    assert_eq!(product.name, "Sample Theme");
    assert_eq!(product.product_type, ProductType::Theme);
}

#[tokio::test]
async fn existing_working_copy_must_be_the_root() {
    let Some(repo) = LocalSvnRepository::new_or_skip() else {
        return;
    };
    let work = TempDir::new();
    let service = service();
    repo.create_plugin("sample-plugin", Some("1.0.0"), "Sample Plugin")
        .unwrap();
    let checkout = work.path("wc");
    repo.run(&strings(&[
        "checkout",
        &format!("{}/sample-plugin", repo.url()),
        &checkout.to_string_lossy(),
    ]))
    .unwrap();
    let error = service
        .describe_working_copy(&checkout.join("trunk"))
        .await
        .unwrap_err();
    assert!(
        matches!(error, RepositorySetupError::NotWorkingCopyRoot { .. }),
        "{error:?}"
    );
    work.mkdir("plain");
    let error = service.describe_working_copy(&work.path("plain")).await.unwrap_err();
    assert!(matches!(error, RepositorySetupError::NotAWorkingCopy(_)), "{error:?}");
}

// MARK: Rust-only

#[tokio::test]
async fn local_changes_are_listed_relative_to_the_root() {
    let Some(repo) = LocalSvnRepository::new_or_skip() else {
        return;
    };
    let work = TempDir::new();
    let service = service();
    repo.create_plugin("sample-plugin", Some("1.0.0"), "Sample Plugin")
        .unwrap();
    let checkout = work.path("wc");
    repo.run(&strings(&[
        "checkout",
        &format!("{}/sample-plugin", repo.url()),
        &checkout.to_string_lossy(),
    ]))
    .unwrap();
    work.write("wc/trunk/readme.txt", "changed");
    work.write("wc/trunk/new.php", "<?php");
    work.write("wc/trunk/.DS_Store", "junk");
    let description = service.describe_working_copy_locally(&checkout).await.unwrap();
    let mut changes = description.local_changes.clone();
    changes.sort();
    let sep = std::path::MAIN_SEPARATOR;
    assert_eq!(
        changes,
        vec![
            format!("modified: trunk{sep}readme.txt"),
            format!("unversioned: trunk{sep}new.php"),
        ]
    );
    assert_eq!(description.repository.product_type, Some(ProductType::Plugin));
    assert_eq!(description.repository.versions, vec!["1.0.0".to_owned()]);
    assert_eq!(description.repository.product_name.as_deref(), Some("Sample Plugin"));
}

#[tokio::test]
async fn empty_repository_asks_for_the_type() {
    let Some(repo) = LocalSvnRepository::new_or_skip() else {
        return;
    };
    let service = service();
    repo.run(&strings(&[
        "mkdir",
        "-m",
        "Create",
        &format!("{}/empty-product", repo.url()),
    ]))
    .unwrap();
    let url = format!("{}/empty-product", repo.url());
    let description = service.describe_remote(&url, None).await.unwrap();
    assert_eq!(description.product_type, None);
    assert_eq!(
        description.notes,
        vec!["The repository is empty; choose Plugin or Theme.".to_owned()]
    );
    let theme = service.describe_remote(&url, Some(ProductType::Theme)).await.unwrap();
    assert_eq!(theme.product_type, Some(ProductType::Theme));
    assert!(
        theme
            .notes
            .iter()
            .any(|n| n.starts_with("No released version directory exists."))
    );
}

// MARK: LiveRepositoryTests

/// Opt-in, read-only checks against WordPress.org and a local checkout:
/// `RELEASEDOCK_LIVE_SLUG=tlp-team RELEASEDOCK_LIVE_WC=~/svn/tlp-team cargo test -p rd-integrations --test repository_setup -- --ignored`
#[tokio::test]
#[ignore = "reads plugins.svn.wordpress.org; needs RELEASEDOCK_LIVE_SLUG"]
async fn describe_real_repository_and_checkout() {
    let Ok(slug) = std::env::var("RELEASEDOCK_LIVE_SLUG") else {
        eprintln!("skipped: set RELEASEDOCK_LIVE_SLUG");
        return;
    };
    let service = service();
    let start = std::time::Instant::now();
    let remote = service
        .describe_remote(&format!("https://plugins.svn.wordpress.org/{slug}"), None)
        .await
        .unwrap();
    println!(
        "LIVE remote: {:?} name={:?} version={:?} tags={} notes={:?}",
        start.elapsed(),
        remote.product_name,
        remote.current_version,
        remote.versions.len(),
        remote.notes
    );
    assert!(remote.product_name.is_some());
    assert!(remote.current_version.is_some());

    if let Ok(wc) = std::env::var("RELEASEDOCK_LIVE_WC") {
        let wc = match wc.strip_prefix("~/") {
            Some(rest) => std::path::PathBuf::from(rd_svn::process_runner::home_directory()).join(rest),
            None => std::path::PathBuf::from(wc),
        };
        let start = std::time::Instant::now();
        let local = service.describe_working_copy_locally(&wc).await.unwrap();
        println!(
            "LIVE local: {:?} name={:?} version={:?} changes={}",
            start.elapsed(),
            local.repository.product_name,
            local.repository.current_version,
            local.local_changes.len()
        );
        assert!(start.elapsed() < std::time::Duration::from_secs(3));
    }
}
