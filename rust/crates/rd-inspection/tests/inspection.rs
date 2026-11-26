//! Ported from InspectionTests.swift (ZIPInspectionTests, FolderInspectionTests, the ZIP
//! part of ProductDetectorTests) and BuildTests.swift (ExportIgnoreTests).

mod common;

use common::fixtures::{self, Plugin};
use common::{Entry, TempDir, TestZipBuilder, subpaths, zip_folder};
use rd_inspection::{
    FolderInspecting, FolderInspectionService, GlobPattern, InspectionOptions, ZipInspecting, ZipInspectionService,
};
use rd_model::{InspectionIssueKind as Kind, PackageInspection};
use rd_support::hashing;
use std::collections::HashSet;
use std::path::Path;

// MARK: ZIPInspectionTests

fn inspect(builder: &TestZipBuilder, options: InspectionOptions) -> (TempDir, PackageInspection) {
    let dir = TempDir::new();
    let url = dir.path("p.zip");
    builder.write(&url);
    let result = ZipInspectionService.inspect(&url, &options).expect("inspect");
    (dir, result)
}

fn sample_plugin() -> TestZipBuilder {
    let mut zip = TestZipBuilder::default();
    zip.add_directory("sample-plugin/");
    zip.add(
        "sample-plugin/sample-plugin.php",
        &fixtures::plugin_main_file("Sample Plugin", "1.0.0", "sample-plugin"),
    );
    zip.add(
        "sample-plugin/readme.txt",
        &fixtures::plugin_readme("Sample Plugin", "1.0.0", None),
    );
    zip
}

fn has_blocking(result: &PackageInspection, predicate: impl Fn(&rd_model::InspectionIssue) -> bool) -> bool {
    result.blocking_issues().into_iter().any(predicate)
}

#[test]
fn clean_archive() {
    let (_dir, result) = inspect(&sample_plugin(), InspectionOptions::default());
    assert_eq!(result.archive_root_name.as_deref(), Some("sample-plugin"));
    assert!(result.blocking_issues().is_empty(), "{:?}", result.blocking_issues());
    let files: HashSet<&str> = result.included_files().iter().map(|e| e.path.as_str()).collect();
    assert_eq!(files, HashSet::from(["sample-plugin.php", "readme.txt"]));
    assert_eq!(result.sha256.len(), 64);
}

#[test]
fn real_deflate_archive_extracts_with_matching_hashes() {
    let dir = TempDir::new();
    let a5000 = "A".repeat(5000);
    let plugin = Plugin::new("1.2.0")
        .extra(&[
            (".DS_Store", "junk"),
            ("node_modules/x/index.js", "x"),
            ("logo@2x.png", &a5000),
        ])
        .make(&dir);
    let zip_url = dir.path("sample-plugin.zip");
    zip_folder(&plugin, &zip_url);
    let inspection = ZipInspectionService
        .inspect(&zip_url, &InspectionOptions::default())
        .unwrap();
    assert!(
        inspection.blocking_issues().is_empty(),
        "{:?}",
        inspection.blocking_issues()
    );
    assert!(inspection.excluded_paths.contains(&".DS_Store".to_owned()));
    assert!(inspection.excluded_paths.contains(&"node_modules/".to_owned()));
    let destination = dir.path("out");
    let extracted = ZipInspectionService
        .extract(&zip_url, &inspection, &destination)
        .unwrap();
    for entry in extracted.iter().filter(|e| e.kind == rd_model::EntryKind::File) {
        assert_eq!(
            entry.sha256.as_deref(),
            Some(hashing::sha256_file(&plugin.join(&entry.path)).unwrap().as_str()),
            "{}",
            entry.path
        );
    }
    assert!(!destination.join("node_modules").exists());
    assert!(destination.join("logo@2x.png").exists());
}

#[test]
fn path_traversal_is_blocked() {
    let mut zip = sample_plugin();
    zip.add("sample-plugin/../../evil.php", "<?php");
    zip.add("/etc/evil", "x");
    let (_dir, result) = inspect(&zip, InspectionOptions::default());
    assert!(has_blocking(&result, |i| i.kind == Kind::PathTraversal));
    assert!(has_blocking(&result, |i| i.kind == Kind::AbsolutePath));
    let out = TempDir::new();
    assert!(
        ZipInspectionService
            .extract(Path::new(&result.source_path), &result, &out.path("x"))
            .is_err()
    );
}

#[test]
fn symlink_is_blocked() {
    let mut zip = sample_plugin();
    zip.add_symlink("sample-plugin/link.php", "/etc/passwd");
    let (_dir, result) = inspect(&zip, InspectionOptions::default());
    assert!(has_blocking(&result, |i| i.kind == Kind::SymbolicLink
        && i.path.as_deref() == Some("sample-plugin/link.php")));
}

#[test]
fn duplicate_and_case_conflicting_paths() {
    let mut zip = sample_plugin();
    zip.add("sample-plugin/README.TXT", "dup");
    zip.add("sample-plugin/readme.txt", "dup2");
    let (_dir, result) = inspect(&zip, InspectionOptions::default());
    let duplicates = result
        .blocking_issues()
        .iter()
        .filter(|i| i.kind == Kind::DuplicatePath)
        .count();
    assert!(duplicates >= 2, "{duplicates}");
}

#[test]
fn file_used_as_directory_is_blocked() {
    let mut zip = sample_plugin();
    zip.add("sample-plugin/inc", "file");
    zip.add("sample-plugin/inc/x.php", "<?php");
    let (_dir, result) = inspect(&zip, InspectionOptions::default());
    assert!(has_blocking(&result, |i| i.kind == Kind::DuplicatePath));
}

#[test]
fn nested_root_is_blocked() {
    let mut zip = TestZipBuilder::default();
    zip.add(
        "sample-plugin/sample-plugin/sample-plugin.php",
        &fixtures::plugin_main_file("Sample Plugin", "1.0.0", "sample-plugin"),
    );
    let (_dir, result) = inspect(&zip, InspectionOptions::default());
    assert!(has_blocking(&result, |i| i.kind == Kind::NestedRoot));
}

#[test]
fn macos_junk_and_dev_files_are_excluded() {
    let mut zip = sample_plugin();
    zip.add("__MACOSX/sample-plugin/._sample-plugin.php", "junk");
    zip.add("sample-plugin/.DS_Store", "junk");
    zip.add("sample-plugin/.git/config", "[core]");
    zip.add("sample-plugin/.github/workflows/ci.yml", "x");
    zip.add("sample-plugin/empty.php", "");
    let (_dir, result) = inspect(&zip, InspectionOptions::default());
    assert_eq!(
        result.archive_root_name.as_deref(),
        Some("sample-plugin"),
        "__MACOSX must not break root detection"
    );
    assert!(result.blocking_issues().is_empty(), "{:?}", result.blocking_issues());
    let excluded: HashSet<&str> = result.excluded_paths.iter().map(String::as_str).collect();
    assert_eq!(excluded, HashSet::from(["__MACOSX/", ".DS_Store", ".git/", ".github/"]));
    assert!(
        result
            .issues
            .iter()
            .any(|i| i.kind == Kind::ZeroByteFile && i.path.as_deref() == Some("empty.php"))
    );
    assert!(!result.included_files().iter().any(|e| e.path.starts_with(".git")));
}

#[test]
fn forbidden_and_secret_files_block_zip() {
    let mut zip = sample_plugin();
    zip.add("sample-plugin/build.zip", "PK");
    zip.add("sample-plugin/.env", "DB_PASSWORD=x");
    let (_dir, result) = inspect(&zip, InspectionOptions::default());
    assert!(has_blocking(&result, |i| i.kind == Kind::ForbiddenFile
        && i.path.as_deref() == Some("build.zip")));
    assert!(has_blocking(&result, |i| i.kind == Kind::PossibleSecret
        && i.path.as_deref() == Some(".env")));
}

#[test]
fn control_characters_and_non_utf8_names() {
    let mut zip = sample_plugin();
    zip.add("sample-plugin/bad\u{7}name.php", "x");
    let mut raw = Entry::new("sample-plugin/x", b"x");
    raw.flags = 0;
    zip.entries.push(raw);
    let (_dir, result) = inspect(&zip, InspectionOptions::default());
    assert!(has_blocking(&result, |i| i.kind == Kind::UnsafeFilename));
}

#[test]
fn encrypted_entry_is_blocked() {
    let mut zip = sample_plugin();
    let mut entry = Entry::new("sample-plugin/secret.php", b"x");
    entry.flags = 0x0801;
    zip.entries.push(entry);
    let (_dir, result) = inspect(&zip, InspectionOptions::default());
    assert!(has_blocking(&result, |i| i.kind == Kind::EncryptedEntry));
}

#[test]
fn local_header_name_mismatch_is_corrupt() {
    let mut zip = sample_plugin();
    let mut entry = Entry::new("sample-plugin/ok.php", b"x");
    entry.local_name = Some("sample-plugin/../evil.php".into());
    zip.entries.push(entry);
    let (_dir, result) = inspect(&zip, InspectionOptions::default());
    assert!(has_blocking(&result, |i| i.kind == Kind::CorruptArchive));
}

#[test]
fn huge_zip64_offsets_are_rejected_without_crashing() {
    let mut data = sample_plugin().build();
    // Turn the EOCD into a ZIP64 marker and insert a locator pointing near Int.max.
    let eocd = data.len() - 22;
    data[eocd + 16..eocd + 20].copy_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF]);
    let mut locator = vec![0x50, 0x4B, 0x06, 0x07, 0, 0, 0, 0];
    locator.extend_from_slice(&((i64::MAX - 2) as u64).to_le_bytes());
    locator.extend_from_slice(&[1, 0, 0, 0]);
    data.splice(eocd..eocd, locator);
    let dir = TempDir::new();
    let url = dir.path("evil.zip");
    std::fs::write(&url, data).unwrap();
    let result = ZipInspectionService
        .inspect(&url, &InspectionOptions::default())
        .unwrap();
    assert!(has_blocking(&result, |i| i.kind == Kind::CorruptArchive));
}

#[test]
fn invalid_zip() {
    let dir = TempDir::new();
    let url = dir.write("broken.zip", "this is not a zip archive at all");
    let result = ZipInspectionService
        .inspect(&url, &InspectionOptions::default())
        .unwrap();
    assert!(has_blocking(&result, |i| i.kind == Kind::CorruptArchive));
}

#[test]
fn size_limits() {
    let mut zip = sample_plugin();
    zip.add("sample-plugin/big.txt", &"a".repeat(4096));
    let options = InspectionOptions {
        maximum_total_size: 1024,
        maximum_file_size: 2048,
        ..InspectionOptions::default()
    };
    let (_dir, result) = inspect(&zip, options);
    assert!(has_blocking(&result, |i| i.kind == Kind::ArchiveTooLarge));
}

#[test]
fn extraction_refuses_changed_archive() {
    let dir = TempDir::new();
    let url = dir.path("p.zip");
    sample_plugin().write(&url);
    let inspection = ZipInspectionService
        .inspect(&url, &InspectionOptions::default())
        .unwrap();
    let mut changed = sample_plugin();
    changed.add("sample-plugin/new.php", "<?php");
    changed.write(&url);
    assert!(
        ZipInspectionService
            .extract(&url, &inspection, &dir.path("out"))
            .is_err()
    );
}

// MARK: FolderInspectionTests

#[test]
fn folder_inspection_excludes_artifacts_and_hashes_files() {
    let dir = TempDir::new();
    let root = Plugin::new("1.0.0")
        .extra(&[
            (".git/HEAD", "ref: refs/heads/main"),
            (".gitignore", "node_modules"),
            ("node_modules/pkg/index.js", "x"),
            (".env", "SECRET=1"),
            ("package-lock.json", "{}"),
            ("debug.log", "x"),
            ("empty.txt", ""),
            ("tests/test-x.php", "<?php"),
        ])
        .make(&dir);
    let before = subpaths(&root);
    let options = InspectionOptions {
        exclude_patterns: vec!["/tests".into()],
        ..InspectionOptions::default()
    };
    let result = FolderInspectionService.inspect(&root, &options).unwrap();
    let after = subpaths(&root);
    assert_eq!(before, after, "inspection must not modify the folder");
    let files: HashSet<&str> = result.included_files().iter().map(|e| e.path.as_str()).collect();
    assert!(files.contains("sample-plugin.php"));
    assert!(!files.contains(".env"));
    assert!(
        !files
            .iter()
            .any(|f| f.starts_with(".git") || f.starts_with("node_modules") || f.starts_with("tests"))
    );
    assert!(!files.contains("package-lock.json"));
    assert!(!files.contains("debug.log"));
    assert!(
        result
            .issues
            .iter()
            .any(|i| i.kind == Kind::PossibleSecret && i.excluded)
    );
    assert!(result.issues.iter().any(|i| i.kind == Kind::ZeroByteFile));
    assert!(result.blocking_issues().is_empty(), "{:?}", result.blocking_issues());
    assert!(result.included_files().iter().all(|e| e.sha256.is_some()));
}

#[cfg(unix)]
#[test]
fn symlinks_are_blocked() {
    let dir = TempDir::new();
    let root = fixtures::make_plugin(&dir, "1.0.0");
    std::os::unix::fs::symlink("/etc/hosts", root.join("link.php")).unwrap();
    let result = FolderInspectionService
        .inspect(&root, &InspectionOptions::default())
        .unwrap();
    assert!(has_blocking(&result, |i| i.kind == Kind::SymbolicLink
        && i.path.as_deref() == Some("link.php")));
}

#[test]
fn distignore_is_applied() {
    let dir = TempDir::new();
    let root = Plugin::new("1.0.0")
        .extra(&[
            (".distignore", "# comment\n/src\n*.ts\n"),
            ("src/app.ts", "x"),
            ("lib/x.ts", "y"),
        ])
        .make(&dir);
    let result = FolderInspectionService
        .inspect(&root, &InspectionOptions::default())
        .unwrap();
    let files: HashSet<&str> = result.included_files().iter().map(|e| e.path.as_str()).collect();
    assert!(!files.contains("src/app.ts"));
    assert!(!files.contains("lib/x.ts"));
    assert!(!files.contains(".distignore"));
}

#[test]
fn glob_patterns() {
    let g = |p: &str| GlobPattern::new(p).unwrap();
    assert!(g("*.log").matches("a/b/debug.log", false));
    assert!(g("/build").matches("build", true));
    assert!(!g("/build").matches("src/build", true));
    assert!(g("docs/").matches("x/docs", true));
    assert!(!g("docs/").matches("docs", false));
    assert!(g("**/fixtures").matches("tests/data/fixtures", true));
    assert_eq!(GlobPattern::new("!keep.php"), None);
    assert_eq!(GlobPattern::new("# c"), None);
}

// MARK: ProductDetectorTests (ZIP part)

/// Swift: `testVersionNeverComesFromFileName`. The final assertion runs `ProductDetector`,
/// which lives in rd-versioning; here the extracted main file is checked directly.
#[test]
fn version_never_comes_from_file_name() {
    let dir = TempDir::new();
    let plugin = fixtures::make_plugin(&dir, "1.0.0");
    let zip = dir.path("sample-plugin-9.9.9.zip");
    zip_folder(&plugin, &zip);
    let inspection = ZipInspectionService
        .inspect(&zip, &InspectionOptions::default())
        .unwrap();
    let out = dir.path("x");
    ZipInspectionService.extract(&zip, &inspection, &out).unwrap();
    let main = std::fs::read_to_string(out.join("sample-plugin.php")).unwrap();
    assert!(main.contains(" * Version: 1.0.0\n"));
    assert!(!main.contains("9.9.9"));
}

// MARK: ExportIgnoreTests (BuildTests.swift)

#[test]
fn parses_export_ignore() {
    let text = "# Release exclusions
/tests export-ignore
*.md   export-ignore
/.github export-ignore
*.php text eol=lf
phpunit.xml.dist export-ignore -diff";
    assert_eq!(
        FolderInspectionService::export_ignore_patterns(text),
        ["/tests", "*.md", "/.github", "phpunit.xml.dist"]
    );
}

#[test]
fn gitattributes_apply_only_without_distignore() {
    let dir = TempDir::new();
    let root = Plugin::new("1.1.0")
        .extra(&[
            (
                ".gitattributes",
                "/tests export-ignore\nCONTRIBUTING.md export-ignore\n",
            ),
            ("tests/test-a.php", "<?php"),
            ("CONTRIBUTING.md", "x"),
        ])
        .make(&dir);
    let paths = |root: &Path| -> Vec<String> {
        FolderInspectionService
            .inspect(root, &InspectionOptions::default())
            .unwrap()
            .included_files()
            .iter()
            .map(|e| e.path.clone())
            .collect()
    };
    let found = paths(&root);
    assert!(!found.contains(&"tests/test-a.php".to_owned()));
    assert!(!found.contains(&"CONTRIBUTING.md".to_owned()));

    dir.write("sample-plugin/.distignore", "/vendor\n");
    let found = paths(&root);
    assert!(
        found.contains(&"tests/test-a.php".to_owned()),
        ".distignore takes precedence"
    );
}
