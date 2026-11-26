//! Ported from PerformanceTests.swift. Opt-in scale check:
//! `RELEASEDOCK_PERF_TESTS=1 cargo test -p rd-inspection --release --test performance -- --nocapture`

mod common;

use common::{TempDir, fixtures, zip_folder};
use rd_inspection::{
    FolderInspecting, FolderInspectionService, InspectionOptions, ZipInspecting, ZipInspectionService,
};
use std::time::Instant;

#[test]
fn large_folder_and_zip_inspection() {
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
    let mut start = Instant::now();
    let folder = FolderInspectionService
        .inspect(&root, &InspectionOptions::default())
        .unwrap();
    println!(
        "folder inspection: {} files in {:?}",
        folder.included_files().len(),
        start.elapsed()
    );
    assert_eq!(folder.included_files().len(), 5004);

    let zip = dir.path("big.zip");
    zip_folder(&root, &zip);
    start = Instant::now();
    let inspection = ZipInspectionService
        .inspect(&zip, &InspectionOptions::default())
        .unwrap();
    println!(
        "zip inspection: {} files in {:?}",
        inspection.included_files().len(),
        start.elapsed()
    );
    start = Instant::now();
    let extracted = ZipInspectionService
        .extract(&zip, &inspection, &dir.path("out"))
        .unwrap();
    println!("zip extraction: {} entries in {:?}", extracted.len(), start.elapsed());
    // The candidate-build step (ReleaseStagingService) belongs to the release crate.
}
