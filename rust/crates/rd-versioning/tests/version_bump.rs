//! Ported from VersionBumpTests.swift.
#![allow(clippy::unwrap_used)] // Test helpers outside #[test] functions.

mod common;

use chrono::{Local, TimeZone};
use common::*;
use rd_model::{ProductType, SwiftDate};
use rd_versioning::{Part, ProductDetector, VersionBumpError, VersionBumpService};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

fn plugin(dir: &TempDir, crlf: bool) -> PathBuf {
    let main = plugin_main_file("Sample Plugin", "2.3.0", "sample-plugin")
        + "
define( 'SAMPLE_PLUGIN_VERSION', '2.3.0' );
define( 'SAMPLE_MIN_WP', '2.3.0' ); // not a version constant name";
    let main = if crlf { main.replace('\n', "\r\n") } else { main };
    make_plugin(
        dir,
        "sample-plugin",
        "2.3.0",
        &[
            ("sample-plugin.php", &main),
            (
                "includes/class-plugin.php",
                "<?php\nfinal class Plugin {\n    public const VERSION = '2.3.0';\n    const DB_VERSION = '1.0';\n}\n",
            ),
            ("vendor/lib/x.php", "<?php define( 'LIB_VERSION', '2.3.0' );\n"),
            (
                "package.json",
                "{\n  \"name\": \"sample-plugin\",\n  \"version\": \"2.3.0\",\n  \"devDependencies\": { \"x\": { \"version\": \"2.3.0\" } }\n}\n",
            ),
        ],
    )
}

fn read(root: &Path, file: &str) -> String {
    fs::read_to_string(root.join(file)).unwrap()
}

#[test]
fn suggestions() {
    assert_eq!(
        VersionBumpService::suggest(Part::Patch, "2.3.9").as_deref(),
        Some("2.3.10")
    );
    assert_eq!(
        VersionBumpService::suggest(Part::Minor, "2.3.9").as_deref(),
        Some("2.4.0")
    );
    assert_eq!(
        VersionBumpService::suggest(Part::Major, "2.3").as_deref(),
        Some("3.0.0")
    );
    assert_eq!(
        VersionBumpService::suggest(Part::Patch, "1.0.0-beta1").as_deref(),
        Some("1.0.1")
    );
    assert_eq!(VersionBumpService::suggest(Part::Patch, "1.x"), None);
}

#[test]
fn plans_every_reference_and_applies_them() {
    let dir = TempDir::new();
    let root = plugin(&dir, false);
    let service = VersionBumpService;
    let date = Local.with_ymd_and_hms(2026, 11, 24, 23, 0, 0).unwrap();
    let today = SwiftDate::from_unix_seconds(date.timestamp() as f64);
    let plan = service
        .plan(
            &root,
            &make_product(ProductType::Plugin),
            "2.4.0",
            "New: Lightbox\n* Fix: RTL\n",
            today,
        )
        .unwrap();
    assert_eq!(plan.current_version, "2.3.0");
    let descriptions: BTreeSet<&str> = plan.changes.iter().map(|c| c.description.as_str()).collect();
    assert_eq!(
        descriptions,
        BTreeSet::from([
            "Version header",
            "Stable tag",
            "Changelog entry",
            "Version constant",
            "package.json version"
        ])
    );
    let mut files = plan.files();
    files.sort();
    assert_eq!(
        files,
        [
            "includes/class-plugin.php",
            "package.json",
            "readme.txt",
            "sample-plugin.php"
        ],
        "vendor is skipped"
    );
    assert_eq!(
        plan.changes
            .iter()
            .filter(|c| c.description == "Version constant")
            .count(),
        2
    );
    assert!(plan.notes.is_empty(), "{:?}", plan.notes);
    assert!(root.exists(), "planning writes nothing");
    assert!(read(&root, "sample-plugin.php").contains("Version: 2.3.0"));

    service.apply(&plan, &root).unwrap();
    let main = read(&root, "sample-plugin.php");
    assert!(main.contains(" * Version: 2.4.0"));
    assert!(main.contains("define( 'SAMPLE_PLUGIN_VERSION', '2.4.0' );"));
    assert!(
        main.contains("define( 'SAMPLE_MIN_WP', '2.3.0' );"),
        "only *VERSION constants"
    );
    let cls = read(&root, "includes/class-plugin.php");
    assert!(cls.contains("public const VERSION = '2.4.0';"));
    assert!(cls.contains("const DB_VERSION = '1.0';"), "other versions untouched");
    let json = read(&root, "package.json");
    assert!(json.contains("\"version\": \"2.4.0\",\n  \"devDependencies\": { \"x\": { \"version\": \"2.3.0\" } }"));
    assert!(read(&root, "vendor/lib/x.php").contains("'2.3.0'"));

    // The result is a valid release: the detector and readme agree on 2.4.0.
    let detected = ProductDetector.detect(&root, None, None, None).unwrap();
    assert_eq!(detected.version.as_deref(), Some("2.4.0"));
    let readme = detected.readme.unwrap();
    assert_eq!(readme.stable_tag().as_deref(), Some("2.4.0"));
    assert_eq!(
        readme.changelog_entry("2.4.0").as_deref(),
        Some("* New: Lightbox\n* Fix: RTL")
    );
    assert!(
        read(&root, "readme.txt").contains("= 2.4.0 - 2026-11-24 ="),
        "local date"
    );
}

#[test]
fn missing_changelog_notes_are_reported_not_invented() {
    let dir = TempDir::new();
    let plan = VersionBumpService
        .plan(
            &plugin(&dir, false),
            &make_product(ProductType::Plugin),
            "2.4.0",
            "",
            SwiftDate::now(),
        )
        .unwrap();
    assert!(!plan.changes.iter().any(|c| c.description == "Changelog entry"));
    assert!(plan.notes.iter().any(|n| n.contains("no changelog entry for 2.4.0")));
}

#[test]
fn rejects_older_or_invalid_versions() {
    let dir = TempDir::new();
    let root = plugin(&dir, false);
    let product = make_product(ProductType::Plugin);
    assert_eq!(
        VersionBumpService.plan(&root, &product, "2.3.0", "", SwiftDate::now()),
        Err(VersionBumpError::NotNewer {
            current: "2.3.0".into(),
            new: "2.3.0".into()
        })
    );
    assert_eq!(
        VersionBumpService.plan(&root, &product, "two", "", SwiftDate::now()),
        Err(VersionBumpError::InvalidVersion("two".into()))
    );
}

#[test]
fn refuses_to_overwrite_files_changed_after_the_preview() {
    let dir = TempDir::new();
    let root = plugin(&dir, false);
    let plan = VersionBumpService
        .plan(&root, &make_product(ProductType::Plugin), "2.4.0", "", SwiftDate::now())
        .unwrap();
    dir.write("sample-plugin/readme.txt", "edited meanwhile");
    assert_eq!(
        VersionBumpService.apply(&plan, &root),
        Err(VersionBumpError::FileChanged("readme.txt".into()))
    );
    assert!(
        read(&root, "sample-plugin.php").contains("Version: 2.3.0"),
        "nothing written"
    );
}

#[test]
fn keeps_windows_line_endings() {
    let dir = TempDir::new();
    let root = plugin(&dir, true);
    let service = VersionBumpService;
    let plan = service
        .plan(&root, &make_product(ProductType::Plugin), "2.3.1", "", SwiftDate::now())
        .unwrap();
    service.apply(&plan, &root).unwrap();
    let main = read(&root, "sample-plugin.php");
    assert!(main.contains(" * Version: 2.3.1\r\n"));
    assert!(!main.replace("\r\n", "").contains('\n'), "no bare LF introduced");
}

#[test]
fn theme_bumps_style_css() {
    let dir = TempDir::new();
    let root = make_theme(&dir, "sample-theme", "1.2.0");
    let theme = make_product(ProductType::Theme);
    let service = VersionBumpService;
    let plan = service
        .plan(&root, &theme, "1.3.0", "Block patterns", SwiftDate::now())
        .unwrap();
    let mut files = plan.files();
    files.sort();
    assert_eq!(files, ["readme.txt", "style.css"]);
    assert!(
        !plan.changes.iter().any(|c| c.description == "Stable tag"),
        "theme readmes have no Stable tag"
    );
    service.apply(&plan, &root).unwrap();
    assert_eq!(
        ProductDetector
            .detect(&root, None, None, None)
            .unwrap()
            .version
            .as_deref(),
        Some("1.3.0")
    );
}

#[cfg(unix)]
#[test]
fn apply_keeps_file_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let dir = TempDir::new();
    let root = plugin(&dir, false);
    let main = root.join("sample-plugin.php");
    fs::set_permissions(&main, fs::Permissions::from_mode(0o755)).unwrap();
    let plan = VersionBumpService
        .plan(&root, &make_product(ProductType::Plugin), "2.4.0", "", SwiftDate::now())
        .unwrap();
    VersionBumpService.apply(&plan, &root).unwrap();
    assert_eq!(fs::metadata(&main).unwrap().permissions().mode() & 0o777, 0o755);
}

#[test]
fn error_messages_match_swift() {
    assert_eq!(
        VersionBumpError::NoProduct("x".into()).to_string(),
        "No plugin or theme found in the folder: x"
    );
    assert_eq!(
        VersionBumpError::InvalidVersion("two".into()).to_string(),
        "\"two\" is not a valid version number."
    );
    assert_eq!(
        VersionBumpError::NotNewer {
            current: "2.3.0".into(),
            new: "2.2.0".into()
        }
        .to_string(),
        "2.2.0 is not newer than the current version 2.3.0."
    );
    assert_eq!(
        VersionBumpError::NothingToChange.to_string(),
        "Nothing to change: no version references were found."
    );
    assert_eq!(
        VersionBumpError::FileChanged("readme.txt".into()).to_string(),
        "readme.txt changed after the preview. Review the bump again."
    );
}
