//! `ReleaseStagingService`, `ReleaseError` and the release ZIP. Swift has no dedicated test
//! file for these (they are exercised through the pipeline tests); the candidate-build step
//! of `PerformanceTests.testLargeFolderAndZIPInspection` is here (opt-in).
#![allow(clippy::unwrap_used)]

mod common;

use common::{TempDir, fixtures};
use rd_inspection::{
    FolderInspecting, FolderInspectionService, InspectionOptions, ZipArchive, ZipInspecting, ZipInspectionService,
};
use rd_integrations::BuildError;
use rd_model::{EntryKind, Id};
use rd_pipeline::{ReleaseError, ReleaseStaging, ReleaseStagingService};
use rd_store::AppPaths;
use rd_support::hashing;
use std::fs;
use std::time::Instant;

#[test]
fn release_error_messages_are_verbatim() {
    let cases: Vec<(ReleaseError, &str)> = vec![
        (ReleaseError::InvalidState("Nope.".into()), "Nope."),
        (ReleaseError::ProductNotFound, "The product no longer exists."),
        (ReleaseError::ReleaseNotFound, "The release record was not found."),
        (
            ReleaseError::ValidationBlocked(2),
            "2 required check(s) failed. Resolve them before publishing.",
        ),
        (
            ReleaseError::AutomationNotAuthorized(vec!["a".into(), "b".into()]),
            "Automatic publishing is not allowed: a; b",
        ),
        (
            ReleaseError::PreviewChanged,
            "The working copy changed after the preview was approved. Review the new preview and approve again.",
        ),
        (
            ReleaseError::RemoteChanged("3 path(s) are out of date".into()),
            "The repository changed on the server: 3 path(s) are out of date. Prepare the release again.",
        ),
        (
            ReleaseError::ReleasePathExists("tags/1.1.0".into()),
            "tags/1.1.0 already exists on the server.",
        ),
        (
            ReleaseError::CandidateChanged("x".into()),
            "The release candidate changed after inspection: x",
        ),
        (
            ReleaseError::WorkingCopyObstructed((1..=7).map(|i| format!("p{i}")).collect()),
            "Working-copy paths changed type and need manual attention: p1, p2, p3, p4, p5",
        ),
        (
            ReleaseError::CredentialsRequired,
            "SVN credentials are required to publish.",
        ),
        (
            ReleaseError::MissingData("preview".into()),
            "Release data missing: preview",
        ),
        (ReleaseError::NotCommitted, "The commit did not reach the repository."),
        (ReleaseError::RestoreNotPossible("Why.".into()), "Why."),
        (
            ReleaseError::ReadmeUpdateChangesCode {
                tag: "tags/1.0.0".into(),
                paths: vec!["a.php".into()],
            },
            "The package differs from the released tags/1.0.0 in 1 file(s) besides the readme (a.php). Publish a new version to ship code changes.",
        ),
        (
            ReleaseError::ReadmeUpdateChangesCode {
                tag: "tags/1.0.0".into(),
                paths: (1..=6).map(|i| format!("f{i}.php")).collect(),
            },
            "The package differs from the released tags/1.0.0 in 6 file(s) besides the readme (f1.php, f2.php, f3.php, f4.php, f5.php, …). Publish a new version to ship code changes.",
        ),
    ];
    for (error, message) in cases {
        assert_eq!(error.to_string(), message);
    }
}

#[test]
fn build_failures_map_onto_release_error() {
    let output = format!("{}tail", "x".repeat(900));
    let error: ReleaseError = BuildError::BuildFailed {
        command: "npm run build".into(),
        exit_code: 3,
        output: output.clone(),
    }
    .into();
    let expected = format!(
        "The build command failed (exit code 3): npm run build\n{}",
        &output[output.len() - 800..]
    );
    assert_eq!(error.to_string(), expected);
    assert!(matches!(error, ReleaseError::BuildFailed { exit_code: 3, .. }));
    let other: ReleaseError = BuildError::Copy("disk full".into()).into();
    assert_eq!(other.to_string(), "disk full");
}

fn staging(dir: &TempDir) -> ReleaseStagingService {
    ReleaseStagingService::new(AppPaths::new(dir.path("data")))
}

#[test]
fn builds_a_candidate_with_verified_hashes() {
    let dir = TempDir::new();
    let root = fixtures::make_plugin(&dir, "1.1.0");
    let inspection = FolderInspectionService::new()
        .inspect(&root, &InspectionOptions::default())
        .unwrap();
    let id = Id::new();
    let staging = staging(&dir);
    let candidate = staging
        .build_candidate(&root, &inspection.included_entries, id)
        .unwrap();
    assert_eq!(
        candidate.root_dir(),
        dir.path("data").join("Staging").join(id.to_string()).join("candidate")
    );
    assert_eq!(
        candidate.tree_digest, inspection.sha256,
        "same digest as the folder inspection"
    );
    for file in candidate.files() {
        let copied = candidate.root_dir().join(&file.path);
        assert_eq!(hashing::sha256_file(&copied).ok(), file.sha256);
        assert_eq!(fs::metadata(&copied).unwrap().len(), file.size);
    }
    assert_eq!(
        staging.recorded_staging_path(id),
        Some(
            dir.path("data/Staging")
                .join(id.to_string())
                .to_string_lossy()
                .into_owned()
        )
    );

    // A file changed after inspection is refused.
    fs::write(root.join("readme.txt"), "changed").unwrap();
    let error = staging
        .build_candidate(&root, &inspection.included_entries, id)
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "The release candidate changed after inspection: readme.txt was modified after inspection"
    );

    staging.remove_workspace(id);
    assert!(!dir.path("data/Staging").join(id.to_string()).exists());
}

#[cfg(unix)]
#[test]
fn a_file_replaced_by_a_symlink_is_refused() {
    let dir = TempDir::new();
    let root = fixtures::make_plugin(&dir, "1.1.0");
    let inspection = FolderInspectionService::new()
        .inspect(&root, &InspectionOptions::default())
        .unwrap();
    fs::remove_file(root.join("includes/class-sample.php")).unwrap();
    std::os::unix::fs::symlink(root.join("readme.txt"), root.join("includes/class-sample.php")).unwrap();
    let error = staging(&dir)
        .build_candidate(&root, &inspection.included_entries, Id::new())
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "The release candidate changed after inspection: includes/class-sample.php is no longer a regular file"
    );
}

#[test]
fn backups_and_manifests_go_to_their_folders() {
    let dir = TempDir::new();
    let staging = staging(&dir);
    let backup = staging.write_backup("# svn status\n", "acme-x-revert.diff").unwrap();
    assert_eq!(backup, dir.path("data/Backups/acme-x-revert.diff"));
    assert_eq!(fs::read_to_string(&backup).unwrap(), "# svn status\n");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(dir.path("data/Backups")).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
    }
}

#[test]
fn release_zip_has_the_slug_folder_and_round_trips() {
    let dir = TempDir::new();
    let root = fixtures::Plugin::new("1.1.0")
        .extra(&[("logo@2x.png", "img"), ("empty.txt", "")])
        .make(&dir);
    let zip = dir.path("out/sample-plugin-1.1.0.zip");
    fs::create_dir_all(zip.parent().unwrap()).unwrap();
    rd_pipeline::release_zip::zip_tree(&root, "sample-plugin", &zip).unwrap();

    let archive = ZipArchive::open(&zip).unwrap();
    let names: Vec<String> = archive.entries.iter().filter_map(|e| e.name.clone()).collect();
    assert!(names.contains(&"sample-plugin/".to_owned()));
    assert!(names.contains(&"sample-plugin/includes/".to_owned()));
    assert!(names.contains(&"sample-plugin/sample-plugin.php".to_owned()));
    assert!(names.iter().all(|n| n.starts_with("sample-plugin/")), "{names:?}");

    // ReleaseDock's own (strict) inspector accepts it and extracts identical files.
    let inspection = ZipInspectionService::new()
        .inspect(&zip, &InspectionOptions::default())
        .unwrap();
    assert!(inspection.blocking_issues().is_empty(), "{:?}", inspection.issues);
    assert_eq!(inspection.archive_root_name.as_deref(), Some("sample-plugin"));
    let out = dir.path("extracted");
    let entries = ZipInspectionService::new().extract(&zip, &inspection, &out).unwrap();
    for entry in entries.iter().filter(|e| e.kind == EntryKind::File) {
        assert_eq!(
            fs::read(out.join(&entry.path)).unwrap(),
            fs::read(root.join(&entry.path)).unwrap(),
            "{}",
            entry.path
        );
    }

    // And so does Info-ZIP, where installed.
    if let Ok(unzip) = which::which("unzip") {
        let status = std::process::Command::new(unzip)
            .arg("-tq")
            .arg(&zip)
            .stdout(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success());
    }
}

/// Opt-in scale check (`PerformanceTests.testLargeFolderAndZIPInspection`, candidate step):
/// `RELEASEDOCK_PERF_TESTS=1 cargo test -p rd-pipeline --release --test staging -- --nocapture`
#[test]
fn large_candidate_build() {
    if std::env::var("RELEASEDOCK_PERF_TESTS").as_deref() != Ok("1") {
        eprintln!("skipped: set RELEASEDOCK_PERF_TESTS=1");
        return;
    }
    let dir = TempDir::new();
    let root = fixtures::make_plugin(&dir, "1.0.0");
    for i in 0..5000 {
        dir.write(
            &format!("sample-plugin/vendor/pkg{}/src/File{i}.php", i % 50),
            format!("<?php\n// file {i}\n{}", "x".repeat(2000)),
        );
    }
    let folder = FolderInspectionService::new()
        .inspect(&root, &InspectionOptions::default())
        .unwrap();
    assert_eq!(folder.included_files().len(), 5004);
    let start = Instant::now();
    let staged = staging(&dir)
        .build_candidate(&root, &folder.included_entries, Id::new())
        .unwrap();
    println!(
        "candidate build: {} files in {:?}",
        staged.files().len(),
        start.elapsed()
    );
}
