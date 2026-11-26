//! `ProductServiceTests` from CoreTests.swift, plus the remaining validation messages.
#![allow(clippy::unwrap_used)]

mod common;

use common::{TempDir, make_plugin, make_product, product_with_working_copy};
use rd_model::{ProductType, ReleaseMode};
use rd_store::*;
use std::sync::Arc;

fn service(dir: &TempDir, data: &str) -> ProductService {
    ProductService::new(Arc::new(Store::in_memory().unwrap()), AppPaths::new(dir.path(data)))
}

fn has_error(issues: &[ProductValidationIssue], field: ProductField) -> bool {
    issues.iter().any(|i| i.field == field && i.is_error)
}

#[test]
fn validation() {
    let dir = TempDir::new();
    let service = service(&dir, "data");
    let mut product = product_with_working_copy(dir.path("wc").to_str().unwrap());
    let issues = service.validate(&product);
    assert!(!issues.iter().any(|i| i.is_error), "{issues:?}");

    product.svn_repository_url = "https://plugins.svn.wordpress.org/other-slug".into();
    assert!(
        has_error(&service.validate(&product), ProductField::SvnUrl),
        "URL must match slug"
    );
    product.svn_repository_url = "https://user:pw@plugins.svn.wordpress.org/sample-plugin".into();
    assert!(
        has_error(&service.validate(&product), ProductField::SvnUrl),
        "no credentials in URL"
    );
    product.svn_repository_url = "http://plugins.svn.wordpress.org/sample-plugin".into();
    assert!(has_error(&service.validate(&product), ProductField::SvnUrl));
    product.svn_repository_url = "https://themes.svn.wordpress.org/sample-plugin".into();
    assert!(
        has_error(&service.validate(&product), ProductField::SvnUrl),
        "plugin on themes host"
    );

    let mut product = product_with_working_copy(dir.path("dev/wc").to_str().unwrap());
    product.local_development_path = Some(dir.path("dev").to_string_lossy().into_owned());
    assert!(
        has_error(&service.validate(&product), ProductField::WorkingCopy),
        "no nesting"
    );
    let product = make_product(
        ProductType::Plugin,
        "Bad Slug",
        None,
        dir.path("wc").to_str().unwrap(),
        ReleaseMode::Assisted,
        false,
    );
    assert!(service.validate(&product).iter().any(|i| i.field == ProductField::Slug));
    assert!(service.save(&product).is_err());
}

#[test]
fn suggestion_from_folder() {
    let dir = TempDir::new();
    let root = make_plugin(&dir, "3.0.0");
    let suggestion = service(&dir, "d").suggestion(&root).unwrap();
    assert_eq!(suggestion.product_type, ProductType::Plugin);
    assert_eq!(suggestion.slug, "sample-plugin");
    assert_eq!(suggestion.version.as_deref(), Some("3.0.0"));
}

// Not in Swift: every other message, verbatim, and the save/delete round trip.

fn messages(service: &ProductService, product: &rd_model::Product) -> Vec<(String, bool)> {
    service
        .validate(product)
        .into_iter()
        .map(|i| (i.message, i.is_error))
        .collect()
}

#[test]
fn validation_messages_are_the_swift_messages() {
    let dir = TempDir::new();
    let service = service(&dir, "data");
    let wc = dir.path("wc").to_string_lossy().into_owned();

    let mut product = product_with_working_copy(&wc);
    product.name = " \t".into();
    product.expected_header_name = String::new();
    product.svn_repository_url = "https://themes.svn.wordpress.org/other".into();
    assert_eq!(
        messages(&service, &product),
        [
            ("Name is required.".to_owned(), true),
            ("Expected Plugin Name is required for identity checks.".to_owned(), true),
            ("Plugins are hosted on plugins.svn.wordpress.org.".to_owned(), true),
            (
                "The WordPress.org URL must be https://plugins.svn.wordpress.org/sample-plugin.".to_owned(),
                true
            ),
        ]
    );

    let mut product = product_with_working_copy(&wc);
    product.svn_repository_url = "file:///tmp/repo".into();
    product.release_mode = ReleaseMode::Automatic;
    assert_eq!(
        messages(&service, &product),
        [
            ("Local file:// repositories are for testing only.".to_owned(), false),
            (
                "Automatic Release mode has no effect until automation is explicitly enabled.".to_owned(),
                false
            ),
        ]
    );

    let mut product = product_with_working_copy("relative/wc");
    product.svn_repository_url = "svn://plugins.svn.wordpress.org/sample-plugin".into();
    assert_eq!(
        messages(&service, &product),
        [
            ("The SVN URL must use https.".to_owned(), true),
            ("The working copy path must be absolute.".to_owned(), true),
        ]
    );

    let product = product_with_working_copy(dir.path("data/Staging/x").to_str().unwrap());
    assert_eq!(
        messages(&service, &product),
        [("The working copy cannot live inside the staging area.".to_owned(), true)]
    );

    let home = dirs::home_dir().unwrap();
    let product = product_with_working_copy(&format!("{}/", home.display()));
    assert_eq!(
        messages(&service, &product),
        [("Choose a dedicated folder for the SVN working copy.".to_owned(), true)]
    );

    let file = dir.write("a-file", "x");
    let product = product_with_working_copy(file.to_str().unwrap());
    assert_eq!(
        messages(&service, &product),
        [("The working copy path is a file.".to_owned(), true)]
    );

    let mut product = make_product(
        ProductType::Theme,
        "sample-theme",
        None,
        &wc,
        ReleaseMode::Assisted,
        false,
    );
    product.local_development_path = Some(dir.path("missing-dev").to_string_lossy().into_owned());
    product.assets_path = Some(dir.path("missing-assets").to_string_lossy().into_owned());
    product.main_plugin_file = Some("inc/main.php".into());
    product.github_repository_url = Some("https://gitlab.com/owner/repo".into());
    product.svn_username = Some(" alice".into());
    assert_eq!(
        messages(&service, &product),
        [
            ("The development folder does not exist.".to_owned(), false),
            ("Assets sync applies to plugins only.".to_owned(), true),
            ("The assets folder does not exist.".to_owned(), true),
            (
                "The main plugin file must be a .php file name at the plugin root.".to_owned(),
                true
            ),
            (
                "Enter a GitHub repository like https://github.com/owner/repo.".to_owned(),
                true
            ),
            ("The SVN username has leading or trailing spaces.".to_owned(), true),
        ]
    );
    assert_eq!(
        service.validate(&product)[0].id(),
        "developmentPath:The development folder does not exist."
    );

    let error = service.save(&product).unwrap_err();
    assert_eq!(
        error.to_string(),
        "Assets sync applies to plugins only.\nThe assets folder does not exist.\nThe main plugin file must be a .php file name at the plugin root.\nEnter a GitHub repository like https://github.com/owner/repo.\nThe SVN username has leading or trailing spaces."
    );
}

#[test]
fn save_updates_the_timestamp_and_delete_removes() {
    let dir = TempDir::new();
    let service = service(&dir, "data");
    let mut product = product_with_working_copy(dir.path("wc").to_str().unwrap());
    product.updated_at = rd_model::SwiftDate(0.0);
    service.save(&product).unwrap();
    let saved = service.all_products().unwrap();
    assert_eq!(saved.len(), 1);
    assert!(saved[0].updated_at.0 > 0.0);
    service.delete(product.id).unwrap();
    assert!(service.all_products().unwrap().is_empty());
    assert_eq!(
        service.default_working_copy_path("acme", ProductType::Theme),
        dir.path("data/Products/theme-acme/svn").to_string_lossy()
    );
}
