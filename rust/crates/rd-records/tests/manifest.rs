//! Ported from `ManifestTests` in `CoreTests.swift`.

mod common;

use common::default_product;
use rd_model::{
    EntryKind, PackageEntry, ReleaseMode, SwiftDate, ValidationCheckId, ValidationReport, ValidationResult,
};
use rd_records::{ReleaseManifest, ReleaseRecord, ReleaseSource, StagedCandidate};

fn sample() -> (ReleaseRecord, StagedCandidate) {
    let mut product = default_product();
    product.svn_username = Some("alice".into());
    let mut record = ReleaseRecord::new(&product, ReleaseSource::zip("/tmp/p.zip"), ReleaseMode::Assisted);
    record.version = Some("1.2.0".into());
    record.source_sha256 = Some("a".repeat(64));
    record.revision_before = Some(100);
    record.committed_revision = Some(101);
    record.validation = Some(ValidationReport {
        results: vec![ValidationResult::pass(ValidationCheckId::VersionValidity, "ok")],
        block_on_warnings: false,
    });
    let candidate = StagedCandidate::new(
        "/tmp/c",
        vec![PackageEntry {
            path: "a.php".into(),
            kind: EntryKind::File,
            size: 3,
            sha256: Some("abc".into()),
        }],
        "digest",
    );
    (record, candidate)
}

#[test]
fn manifest_contains_release_data_and_no_secrets() {
    let (record, candidate) = sample();
    let manifest = ReleaseManifest::make(
        &record,
        Some(&candidate),
        "tags/1.2.0",
        Some("deadbeef"),
        "test",
        SwiftDate::now(),
    );
    let json = String::from_utf8(manifest.encoded().unwrap()).unwrap();
    assert!(json.contains("\"svnRevisionBefore\" : 100"), "{json}");
    assert!(json.contains("\"finalRevision\" : 101"));
    assert!(json.contains("tags/1.2.0"));
    assert!(json.contains("a.php"));
    for forbidden in ["password", "alice", "token"] {
        assert!(
            !json.to_lowercase().contains(forbidden),
            "manifest must not contain {forbidden}"
        );
    }
}

// Rust-only: the exact encoding.

#[test]
fn manifest_is_encoded_like_swift() {
    let (record, candidate) = sample();
    // 2026-11-26T09:30:15Z
    let now = SwiftDate::from_unix_seconds(1_795_685_415.0);
    let manifest = ReleaseManifest::make(&record, Some(&candidate), "tags/1.2.0", None, "test", now);
    let json = String::from_utf8(manifest.encoded().unwrap()).unwrap();
    let expected = format!(
        r#"{{
  "candidateDigest" : "digest",
  "files" : [
    {{
      "path" : "a.php",
      "sha256" : "abc",
      "size" : 3
    }}
  ],
  "finalRevision" : 101,
  "generatedAt" : "2026-11-26T09:30:15Z",
  "generator" : "test",
  "kind" : "version",
  "mode" : "assisted",
  "product" : "Sample Plugin",
  "releaseID" : "{id}",
  "releasePath" : "tags/1.2.0",
  "schemaVersion" : 1,
  "slug" : "sample-plugin",
  "sourceKind" : "zip",
  "sourcePath" : "/tmp/p.zip",
  "sourceSHA256" : "{sha}",
  "state" : "detected",
  "svnRevisionBefore" : 100,
  "svnURL" : "https://plugins.svn.wordpress.org/sample-plugin",
  "type" : "plugin",
  "validation" : [
    {{
      "check" : "versionValidity",
      "message" : "ok",
      "name" : "Version validity",
      "severity" : "info",
      "status" : "passed"
    }}
  ],
  "version" : "1.2.0"
}}"#,
        id = record.id,
        sha = "a".repeat(64)
    );
    assert_eq!(json, expected);

    let decoded: ReleaseManifest = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded, manifest);
}

#[test]
fn manifest_without_candidate_has_no_files() {
    let (mut record, _) = sample();
    record.version = None;
    record.source_sha256 = None;
    let manifest = ReleaseManifest::make(&record, None, "tags/", None, "test", SwiftDate(0.0));
    assert_eq!(manifest.version, "");
    assert_eq!(manifest.source_sha256, "");
    assert_eq!(manifest.candidate_digest, None);
    let json = String::from_utf8(manifest.encoded().unwrap()).unwrap();
    assert!(json.contains("\"files\" : [\n\n  ]"), "{json}");
    assert!(json.contains("\"generatedAt\" : \"2001-01-01T00:00:00Z\""));
}
