//! The release record must read the Swift app's stored JSON and write JSON the Swift app
//! reads back unchanged. The fixtures are real (scrubbed) records from the Swift store;
//! long arrays in them were truncated.

use rd_model::{ReleaseMode, ValidationSeverity};
use rd_records::*;
use serde_json::Value;

const FIXTURES: [(&str, &str); 3] = [
    (
        "release-completed.json",
        include_str!("fixtures/release-completed.json"),
    ),
    ("release-failed.json", include_str!("fixtures/release-failed.json")),
    (
        "release-cancelled.json",
        include_str!("fixtures/release-cancelled.json"),
    ),
];

/// Asserts that `actual` has exactly the keys and values of `expected`. Numbers compare by
/// value, so `3` and `3.0` are equal (Swift writes a whole `Double` without a fraction).
fn assert_same_json(expected: &Value, actual: &Value, path: &str) {
    match (expected, actual) {
        (Value::Object(e), Value::Object(a)) => {
            let missing: Vec<_> = e.keys().filter(|k| !a.contains_key(*k)).collect();
            let extra: Vec<_> = a.keys().filter(|k| !e.contains_key(*k)).collect();
            assert!(
                missing.is_empty() && extra.is_empty(),
                "{path}: missing keys {missing:?}, unexpected keys {extra:?}"
            );
            for (key, value) in e {
                assert_same_json(value, &a[key], &format!("{path}.{key}"));
            }
        }
        (Value::Array(e), Value::Array(a)) => {
            assert_eq!(e.len(), a.len(), "{path}: array length");
            for (index, (e, a)) in e.iter().zip(a).enumerate() {
                assert_same_json(e, a, &format!("{path}[{index}]"));
            }
        }
        (Value::Number(e), Value::Number(a)) => {
            assert_eq!(e.as_f64(), a.as_f64(), "{path}: number");
        }
        _ => assert_eq!(expected, actual, "{path}"),
    }
}

fn decode(name: &str, json: &str) -> ReleaseRecord {
    serde_json::from_str(json).unwrap_or_else(|e| panic!("{name}: {e}"))
}

#[test]
fn decodes_every_stored_release() {
    for (name, json) in FIXTURES {
        let record = decode(name, json);
        assert!(!record.transitions.is_empty(), "{name}");
        assert_eq!(record.transitions[0].to, ReleaseState::Detected, "{name}");
    }
}

#[test]
fn round_trips_to_equal_values() {
    for (name, json) in FIXTURES {
        let record = decode(name, json);
        let encoded = serde_json::to_string(&record).unwrap();
        let again: ReleaseRecord = serde_json::from_str(&encoded).unwrap();
        assert_eq!(again, record, "{name}");
    }
}

#[test]
fn re_encoded_json_has_the_same_keys_and_values() {
    for (name, json) in FIXTURES {
        let original: Value = serde_json::from_str(json).unwrap();
        let record = decode(name, json);
        let encoded = serde_json::to_value(&record).unwrap();
        assert_same_json(&original, &encoded, name);
    }
}

#[test]
fn completed_release_reads_as_the_swift_app_shows_it() {
    let record = decode(FIXTURES[0].0, FIXTURES[0].1);
    assert_eq!(record.state, ReleaseState::Completed);
    assert_eq!(record.kind(), ReleaseKind::Version);
    assert_eq!(record.mode, ReleaseMode::Assisted);
    assert_eq!(record.source.kind(), SourceKind::Zip);
    assert_eq!(record.source.path(), "/Users/me/Downloads/radius-booking.1.0.24.zip");
    assert!(record.commit_marker().starts_with("ReleaseDock-Release-ID: "));
    assert_eq!(
        record.commit_marker(),
        format!("ReleaseDock-Release-ID: {}", record.id.to_string().to_uppercase())
    );
    assert_eq!(
        record.completed_at(),
        record.transitions.last().map(|t| t.at),
        "the last transition completes the release"
    );
    assert!(record.is_directory_tracked());
    let status = record.directory_status.as_ref().unwrap();
    assert_eq!(status.state, DirectoryState::Live);
    assert!(!record.awaits_directory(SwiftDate::now(), DIRECTORY_WAIT_WINDOW));
    assert!(record.verification.as_ref().unwrap().is_clean());
    assert_eq!(record.approval.as_ref().unwrap().kind, ApprovalKind::User);
    assert_eq!(
        record.approval.as_ref().unwrap().preview_fingerprint,
        record.preview.as_ref().unwrap().fingerprint
    );
    let repository = record.repository.as_ref().unwrap();
    assert_eq!(repository.authentication, AuthenticationAvailability::CachedBySvn);
    assert_eq!(repository.latest_wordpress_version.as_deref(), Some("7.1.5"));
    let detected = record.detected.as_ref().unwrap();
    assert_eq!(detected.version.as_deref(), Some("1.0.24"));
    let candidate = record.candidate.as_ref().unwrap();
    assert!(candidate.files().iter().all(|f| f.kind == EntryKind::File));
    let checks = record.code_checks.as_ref().unwrap();
    assert!(!checks.plugin_check_ran);
    assert!(checks.findings_for(CodeCheckTool::PhpLint).is_empty());
}

#[test]
fn failed_and_cancelled_releases_keep_their_errors() {
    let failed = decode(FIXTURES[1].0, FIXTURES[1].1);
    assert_eq!(failed.state, ReleaseState::Failed);
    assert!(failed.state.is_terminal());
    assert_eq!(failed.completed_at(), None);
    assert!(!failed.is_directory_tracked());
    let cancelled = decode(FIXTURES[2].0, FIXTURES[2].1);
    assert_eq!(cancelled.state, ReleaseState::Cancelled);
    assert!(!cancelled.state.may_have_committed());
}

/// Fields the fixtures do not contain, written the way Swift's synthesized `Codable` does.
const SWIFT_RECORD_WITH_EVERY_FIELD: &str = r#"{
  "approval": {"at": 817293378.5, "kind": "automatic", "previewFingerprint": "fp"},
  "assets": {"blueprintProblem": "not JSON", "files": [{"height": 250, "path": "banner-772x250.png", "size": 3, "width": 772}, {"path": "readme.txt", "size": 1}]},
  "build": {"builtPath": "/tmp/build/acme", "command": "npm run build", "durationSeconds": 3, "exitCode": 0, "outputTail": "done"},
  "codeChecks": {"findings": [{"code": "WordPress.Security", "file": "acme.php", "line": 12, "message": "Escape output", "severity": "warning", "tool": "pluginCheck"}, {"message": "Syntax error", "severity": "error", "tool": "phpLint"}], "lintedFiles": 2, "phpLintSkipped": "no php", "pluginCheckRan": true},
  "commitTemplate": "Tagging {version}",
  "committedRevision": 12,
  "createdAt": 817293326.03244,
  "directoryStatus": {"checkedAt": 817293751.25, "state": "notListed"},
  "errors": ["boom"],
  "gitCommit": "deadbeef",
  "githubRelease": {"alreadyExisted": true, "assetName": "acme.zip", "at": 817293800, "notes": ["n"], "tag": "v1.2.0", "url": "https://github.com/acme/acme/releases/tag/v1.2.0"},
  "guidance": ["Confirm the release"],
  "id": "34629C18-132D-4671-9B07-748F19721D10",
  "mode": "automatic",
  "productID": "EE90AEAA-DAB2-45F4-805D-58DE936B984C",
  "productName": "Acme",
  "productSlug": "acme",
  "productType": "theme",
  "releaseZipPath": "/tmp/acme.zip",
  "repository": {"authentication": "none", "connectivityError": "offline", "existingVersions": [], "notes": [], "outOfDatePaths": ["trunk"], "structureProblems": ["no trunk"], "workingCopyChanges": ["M a"], "workingCopyExists": false, "workingCopyURLMismatch": "https://example.org/other"},
  "restoresVersion": "1.0.0",
  "source": {"folder": {"path": "/Users/me/dev/acme"}},
  "state": "recoveryRequired",
  "svnURL": "https://themes.svn.wordpress.org/acme",
  "transitions": [{"at": 817293326.03244, "to": "detected"}, {"at": 817293327, "from": "detected", "note": "n", "to": "recoveryRequired"}],
  "updatedAt": 817293327,
  "verification": {"checksumMismatches": ["a.php"], "missingFiles": [], "notes": [], "tagExists": true, "tagURL": "https://themes.svn.wordpress.org/acme/1.2.0", "unexpectedFiles": []},
  "verifiedNotCommitted": true,
  "workingCopyModified": true
}"#;

#[test]
fn reads_and_writes_every_optional_field() {
    let original: Value = serde_json::from_str(SWIFT_RECORD_WITH_EVERY_FIELD).unwrap();
    let record: ReleaseRecord = serde_json::from_str(SWIFT_RECORD_WITH_EVERY_FIELD).unwrap();
    assert_same_json(&original, &serde_json::to_value(&record).unwrap(), "record");

    assert_eq!(record.source, ReleaseSource::folder("/Users/me/dev/acme"));
    assert_eq!(record.release_kind, None, "records from before readme updates");
    assert_eq!(record.kind(), ReleaseKind::Version);
    assert_eq!(record.state, ReleaseState::RecoveryRequired);
    assert!(record.state.may_have_committed());
    assert_eq!(record.state.display_name(), "Recovery Required");
    assert_eq!(record.approval.as_ref().unwrap().kind, ApprovalKind::Automatic);
    let github = record.github_release.as_ref().unwrap();
    assert!(github.succeeded());
    assert!(github.already_existed);
    let build = record.build.as_ref().unwrap();
    assert_eq!(build.duration_seconds, 3.0);
    assert_eq!(build.exit_code, 0);
    let assets = record.assets.as_ref().unwrap();
    assert_eq!(assets.files[0].width, Some(772));
    let checks = record.code_checks.as_ref().unwrap();
    assert_eq!(checks.findings_for(CodeCheckTool::PluginCheck).len(), 1);
    assert_eq!(
        checks.findings_for(CodeCheckTool::PhpLint)[0].severity,
        ValidationSeverity::Error
    );
    let repository = record.repository.as_ref().unwrap();
    assert_eq!(repository.authentication, AuthenticationAvailability::None);
    assert_eq!(repository.release_path_exists, None);
    let verification = record.verification.as_ref().unwrap();
    assert_eq!(verification.stable_tag_matches, None);
    assert!(!verification.is_clean(), "checksum mismatch");
    assert_eq!(
        record.directory_status.as_ref().unwrap().state,
        DirectoryState::NotListed
    );

    let value = serde_json::to_value(&record).unwrap();
    assert_eq!(
        value["source"],
        serde_json::json!({"folder": {"path": "/Users/me/dev/acme"}})
    );
    assert!(value.get("releaseKind").is_none(), "absent optionals stay absent");
}

#[test]
fn a_record_without_a_required_key_is_rejected_like_swift() {
    let mut value: Value = serde_json::from_str(SWIFT_RECORD_WITH_EVERY_FIELD).unwrap();
    value.as_object_mut().unwrap().remove("workingCopyModified");
    assert!(serde_json::from_value::<ReleaseRecord>(value).is_err());
}

#[test]
fn new_records_encode_like_swift() {
    let product = rd_model::Product::new(
        "Sample Plugin",
        "sample-plugin",
        rd_model::ProductType::Plugin,
        "https://plugins.svn.wordpress.org/sample-plugin",
        "/tmp/wc",
    );
    let record = ReleaseRecord::new_with(
        Id::parse("34629c18-132d-4671-9b07-748f19721d10").unwrap(),
        &product,
        ReleaseSource::zip("/x.zip"),
        ReleaseMode::DryRun,
        ReleaseKind::ReadmeUpdate,
        SwiftDate(817293326.5),
    );
    let value = serde_json::to_value(&record).unwrap();
    assert_eq!(
        value,
        serde_json::json!({
            "createdAt": 817293326.5,
            "errors": [],
            "guidance": [],
            "id": "34629C18-132D-4671-9B07-748F19721D10",
            "mode": "dryRun",
            "productID": product.id.to_string(),
            "productName": "Sample Plugin",
            "productSlug": "sample-plugin",
            "productType": "plugin",
            "releaseKind": "readmeUpdate",
            "source": {"zip": {"path": "/x.zip"}},
            "state": "detected",
            "svnURL": "https://plugins.svn.wordpress.org/sample-plugin",
            "transitions": [{"at": 817293326.5, "to": "detected"}],
            "updatedAt": 817293326.5,
            "verifiedNotCommitted": false,
            "workingCopyModified": false
        })
    );
}

#[test]
fn stored_service_results_round_trip() {
    let git: GitRepositoryState = serde_json::from_str(
        r#"{"branch":"main","exactTag":"v1.2.0","headCommit":"abc","isRepository":true,"remoteURL":"git@github.com:a/b.git","uncommittedChanges":[]}"#,
    )
    .unwrap();
    assert!(git.is_clean());
    assert_eq!(git.remote_url.as_deref(), Some("git@github.com:a/b.git"));
    assert_eq!(
        serde_json::to_value(&git).unwrap()["remoteURL"],
        "git@github.com:a/b.git"
    );
    assert_eq!(
        serde_json::to_value(GitRepositoryState::not_a_repository()).unwrap(),
        serde_json::json!({"isRepository": false, "uncommittedChanges": []})
    );

    let listing: DirectoryListing =
        serde_json::from_str(r#"{"lastUpdated":"2026-08-18 11:42pm GMT","tested":"7.1.5","version":"5.7.2"}"#).unwrap();
    assert_eq!(listing.last_updated.as_deref(), Some("2026-08-18 11:42pm GMT"));

    let failed = GitHubReleaseResult {
        error: Some("401".into()),
        ..GitHubReleaseResult::new("v1", SwiftDate(1.0))
    };
    assert!(!failed.succeeded());
    assert!(!GitHubReleaseResult::new("v1", SwiftDate(1.0)).succeeded(), "no URL");
}

#[test]
fn preview_counts() {
    let preview = ReleasePreview {
        changes: vec![
            PreviewChange::copy("tags/1.2.0", ChangeKind::Added, true),
            PreviewChange::new("trunk/a.php", ChangeKind::Added, false),
            PreviewChange::new("trunk/b.php", ChangeKind::Modified, false),
            PreviewChange::new("trunk/c.php", ChangeKind::Replaced, false),
            PreviewChange::new("trunk/d.php", ChangeKind::Deleted, false),
            PreviewChange::new("trunk", ChangeKind::PropertyChanged, true),
        ],
        diff_excerpt: String::new(),
        diff_truncated: false,
        commit_message: String::new(),
        fingerprint: "fp".into(),
        working_copy_revision: None,
        created_at: SwiftDate(0.0),
    };
    assert_eq!(preview.added(), 1);
    assert_eq!(preview.modified(), 2);
    assert_eq!(preview.deleted(), 1);
    assert_eq!(preview.changes[0].id(), "tags/1.2.0");
    let value = serde_json::to_value(&preview.changes[5]).unwrap();
    assert_eq!(
        value,
        serde_json::json!({"isCopy": false, "isDirectory": true, "kind": "propertyChanged", "path": "trunk"})
    );
}
