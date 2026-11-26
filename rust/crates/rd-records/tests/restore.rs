//! `RestoreOptions` (Models/Release.swift).

use rd_records::{RepositorySnapshot, RestoreOptions};

/// Adapted from `RestoreReleaseTests.testOptionsListOlderVersionsAndSuggestAVersionAboveTheHighest`:
/// the Swift test releases 1.0.0 and 1.1.0 to a local repository and asks the pipeline for
/// the options, which are `RestoreOptions(snapshot:)` of that repository's snapshot.
#[test]
fn options_list_older_versions_and_suggest_a_version_above_the_highest() {
    let snapshot = RepositorySnapshot {
        current_version: Some("1.1.0".into()),
        existing_versions: vec!["1.0.0".into(), "1.1.0".into()],
        ..RepositorySnapshot::default()
    };
    let options = RestoreOptions::new(&snapshot);
    assert_eq!(options.current_version.as_deref(), Some("1.1.0"));
    assert_eq!(options.older_versions, ["1.0.0"]);
    assert_eq!(options.suggested_version.as_deref(), Some("1.1.1"));
}

// Rust-only checks of the ported logic.

#[test]
fn older_versions_are_newest_first_and_the_suggestion_is_above_the_highest() {
    let snapshot = RepositorySnapshot {
        current_version: Some("2.0.0".into()),
        existing_versions: vec![
            "1.2.0".into(),
            "1.10.0".into(),
            "2.0.0".into(),
            "2.1.0-beta".into(),
            "1.9".into(),
        ],
        ..RepositorySnapshot::default()
    };
    let options = RestoreOptions::new(&snapshot);
    assert_eq!(options.older_versions, ["1.10.0", "1.9", "1.2.0"]);
    // The highest version may be a pre-release; the suggestion drops the suffix.
    assert_eq!(options.highest_version.as_deref(), Some("2.1.0-beta"));
    assert_eq!(options.suggested_version.as_deref(), Some("2.1.1"));
}

#[test]
fn without_a_current_version_nothing_is_older() {
    let snapshot = RepositorySnapshot {
        existing_versions: vec!["1.0.0".into()],
        ..RepositorySnapshot::default()
    };
    let options = RestoreOptions::new(&snapshot);
    assert!(options.older_versions.is_empty());
    assert_eq!(options.highest_version.as_deref(), Some("1.0.0"));
    assert_eq!(options.suggested_version.as_deref(), Some("1.0.1"));
}
