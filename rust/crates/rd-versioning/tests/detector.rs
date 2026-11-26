//! Ported from InspectionTests.swift (ProductDetectorTests) and ThemeChecksTests.swift
//! (testDetectorReadsScreenshotSize).
#![allow(clippy::unwrap_used)] // Test helpers outside #[test] functions.

mod common;

use common::*;
use rd_model::ProductType;
use rd_versioning::{PixelSize, ProductDetectionError, ProductDetector};

#[test]
fn detects_plugin() {
    let dir = TempDir::new();
    let root = make_plugin(&dir, "sample-plugin", "2.3.4", &[]);
    let detected = ProductDetector.detect(&root, None, None, None).unwrap();
    assert_eq!(detected.product_type, ProductType::Plugin);
    assert_eq!(detected.name.as_deref(), Some("Sample Plugin"));
    assert_eq!(detected.version.as_deref(), Some("2.3.4"));
    assert_eq!(detected.main_file.as_deref(), Some("sample-plugin.php"));
    assert_eq!(detected.readme.unwrap().stable_tag().as_deref(), Some("2.3.4"));
    assert_eq!(detected.readme_file.as_deref(), Some("readme.txt"));
}

#[test]
fn ambiguous_main_file() {
    let dir = TempDir::new();
    let other = plugin_main_file("Other", "9.0", "sample-plugin");
    let root = make_plugin(&dir, "sample-plugin", "1.0", &[("other.php", &other)]);
    let detector = ProductDetector;
    let by_slug = detector.detect(&root, None, None, Some("sample-plugin")).unwrap();
    assert_eq!(by_slug.main_file.as_deref(), Some("sample-plugin.php"));
    assert_eq!(
        by_slug.problems,
        ["Multiple files declare \"Plugin Name:\" (other.php, sample-plugin.php); using sample-plugin.php."]
    );
    let ambiguous = detector.detect(&root, None, None, Some("nomatch")).unwrap();
    assert_eq!(ambiguous.main_file, None);
    assert!(!ambiguous.problems.is_empty());
    assert_eq!(
        ambiguous.problems,
        [
            "Multiple files declare \"Plugin Name:\" (other.php, sample-plugin.php). Set the main plugin file in the product settings."
        ]
    );
    assert_eq!(
        detector
            .detect(&root, None, Some("other.php"), None)
            .unwrap()
            .version
            .as_deref(),
        Some("9.0")
    );
    let missing = detector.detect(&root, None, Some("missing.php"), None).unwrap();
    assert_eq!(
        missing.problems,
        ["Configured main plugin file \"missing.php\" has no \"Plugin Name:\" header."]
    );
}

#[test]
fn detects_theme() {
    let dir = TempDir::new();
    let root = make_theme(&dir, "sample-theme", "1.4");
    let detected = ProductDetector.detect(&root, None, None, None).unwrap();
    assert_eq!(detected.product_type, ProductType::Theme);
    assert_eq!(detected.version.as_deref(), Some("1.4"));
    assert_eq!(detected.name.as_deref(), Some("Sample Theme"));
    assert_eq!(detected.main_file.as_deref(), Some("style.css"));
    assert_eq!(detected.screenshot_file.as_deref(), Some("screenshot.png"));
    assert_eq!(detected.screenshot_size, None, "the fixture screenshot is not an image");
    assert_eq!(detected.parent_theme(), None);
}

#[test]
fn nothing_detected() {
    let dir = TempDir::new();
    dir.write("x/readme.txt", "hello");
    assert_eq!(
        ProductDetector.detect(&dir.path("x"), None, None, None),
        Err(ProductDetectionError::NoProductFound)
    );
    assert!(matches!(
        ProductDetector.detect(&dir.path("missing"), None, None, None),
        Err(ProductDetectionError::NotADirectory(_))
    ));
}

#[test]
fn plugin_and_theme_need_an_expected_type() {
    let dir = TempDir::new();
    let root = make_plugin(
        &dir,
        "both",
        "1.0",
        &[("style.css", &theme_style("Sample Theme", "2.0"))],
    );
    assert_eq!(
        ProductDetector.detect(&root, None, None, None),
        Err(ProductDetectionError::AmbiguousType)
    );
    let theme = ProductDetector
        .detect(&root, Some(ProductType::Theme), None, None)
        .unwrap();
    assert_eq!(theme.version.as_deref(), Some("2.0"));
    let plugin = ProductDetector
        .detect(&root, Some(ProductType::Plugin), None, None)
        .unwrap();
    assert_eq!(plugin.version.as_deref(), Some("1.0"));
}

/// Swift extracts a ZIP named `sample-plugin-9.9.9.zip`; the ZIP part belongs to rd-inspection,
/// so this checks the same intent with a folder named after another version.
#[test]
fn version_never_comes_from_file_name() {
    let dir = TempDir::new();
    let root = make_plugin(&dir, "sample-plugin-9.9.9", "1.0.0", &[]);
    assert_eq!(
        ProductDetector
            .detect(&root, None, None, None)
            .unwrap()
            .version
            .as_deref(),
        Some("1.0.0")
    );
}

#[test]
fn detector_reads_screenshot_size() {
    let dir = TempDir::new();
    let root = make_theme(&dir, "sample-theme", "1.0.0");
    write_png(&root.join("screenshot.png"), 880, 660);
    let detected = ProductDetector.detect(&root, None, None, None).unwrap();
    assert_eq!(detected.screenshot_file.as_deref(), Some("screenshot.png"));
    assert_eq!(detected.screenshot_size, Some(PixelSize::new(880, 660)));
}

#[test]
fn error_messages_match_swift() {
    assert_eq!(
        ProductDetectionError::NoProductFound.to_string(),
        "No WordPress plugin (PHP file with a \"Plugin Name:\" header) or theme (style.css with a \"Theme Name:\" header) was found at the package root."
    );
    assert_eq!(
        ProductDetectionError::AmbiguousType.to_string(),
        "The package contains both a plugin header and a theme style.css header."
    );
    assert_eq!(
        ProductDetectionError::NotADirectory("/x".into()).to_string(),
        "Not a directory: /x"
    );
}
