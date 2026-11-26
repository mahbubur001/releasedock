//! Ported from `ReleaseStateMachineTests` in `CoreTests.swift`.

mod common;

use common::default_product;
use rd_model::ReleaseMode;
use rd_records::{ReleaseRecord, ReleaseSource, ReleaseState, ReleaseStateError, ReleaseStateMachine};

#[test]
fn happy_path() {
    let mut record = ReleaseRecord::new(&default_product(), ReleaseSource::zip("/x.zip"), ReleaseMode::Assisted);
    for state in [
        ReleaseState::Inspected,
        ReleaseState::Validated,
        ReleaseState::Staged,
        ReleaseState::Previewed,
        ReleaseState::Approved,
        ReleaseState::Publishing,
        ReleaseState::Verifying,
        ReleaseState::Completed,
    ] {
        ReleaseStateMachine::transition(&mut record, state, None).unwrap();
    }
    assert_eq!(record.state, ReleaseState::Completed);
    assert_eq!(record.transitions.len(), 9);
    assert_eq!(record.transitions.last().unwrap().from, Some(ReleaseState::Verifying));
}

#[test]
fn invalid_transitions() {
    let mut record = ReleaseRecord::new(&default_product(), ReleaseSource::zip("/x.zip"), ReleaseMode::Assisted);
    assert!(ReleaseStateMachine::transition(&mut record, ReleaseState::Publishing, None).is_err());
    assert!(ReleaseStateMachine::transition(&mut record, ReleaseState::Completed, None).is_err());
    assert!(
        !ReleaseStateMachine::can_transition(ReleaseState::Publishing, ReleaseState::Cancelled, ReleaseMode::Assisted),
        "publishing cannot be cancelled"
    );
    assert!(!ReleaseStateMachine::can_transition(
        ReleaseState::Completed,
        ReleaseState::Failed,
        ReleaseMode::Assisted
    ));
    assert!(
        !ReleaseStateMachine::can_transition(ReleaseState::Verifying, ReleaseState::Failed, ReleaseMode::Assisted),
        "a committed release cannot be marked failed"
    );
    assert!(
        !ReleaseStateMachine::can_transition(
            ReleaseState::RecoveryRequired,
            ReleaseState::Publishing,
            ReleaseMode::Assisted
        ),
        "no blind retry"
    );
}

#[test]
fn dry_run_never_publishes() {
    let mut record = ReleaseRecord::new(&default_product(), ReleaseSource::folder("/x"), ReleaseMode::DryRun);
    for state in [
        ReleaseState::Inspected,
        ReleaseState::Validated,
        ReleaseState::Staged,
        ReleaseState::Previewed,
    ] {
        ReleaseStateMachine::transition(&mut record, state, None).unwrap();
    }
    assert_eq!(
        ReleaseStateMachine::transition(&mut record, ReleaseState::Approved, None),
        Err(ReleaseStateError::DryRunCannotPublish)
    );
    ReleaseStateMachine::transition(&mut record, ReleaseState::Completed, None).unwrap();
}

#[test]
fn interruption_mapping() {
    use ReleaseState::*;
    assert_eq!(ReleaseStateMachine::state_after_interruption(Staged), Some(Cancelled));
    assert_eq!(ReleaseStateMachine::state_after_interruption(Approved), Some(Cancelled));
    assert_eq!(
        ReleaseStateMachine::state_after_interruption(Publishing),
        Some(RecoveryRequired)
    );
    assert_eq!(
        ReleaseStateMachine::state_after_interruption(Verifying),
        Some(RecoveryRequired)
    );
    assert_eq!(ReleaseStateMachine::state_after_interruption(Completed), None);
    assert_eq!(ReleaseStateMachine::state_after_interruption(RecoveryRequired), None);
}

#[test]
fn notes_are_redacted() {
    let mut record = ReleaseRecord::new(&default_product(), ReleaseSource::zip("/x.zip"), ReleaseMode::Assisted);
    ReleaseStateMachine::transition(&mut record, ReleaseState::Failed, Some("svn --password hunter2")).unwrap();
    assert!(
        !record
            .transitions
            .last()
            .unwrap()
            .note
            .as_ref()
            .unwrap()
            .contains("hunter2")
    );
}

// Rust-only checks of the ported behaviour.

#[test]
fn errors_use_the_swift_messages() {
    let error = ReleaseStateError::InvalidTransition {
        from: ReleaseState::Detected,
        to: ReleaseState::RecoveryRequired,
    };
    assert_eq!(
        error.to_string(),
        "Invalid release state transition: Detected → Recovery Required"
    );
    assert_eq!(
        ReleaseStateError::DryRunCannotPublish.to_string(),
        "A dry run never publishes."
    );
}

#[test]
fn a_new_record_starts_detected_with_the_product_data() {
    let mut product = default_product();
    product.current_version = Some("1.0.0".into());
    let record = ReleaseRecord::new(&product, ReleaseSource::zip("/x.zip"), ReleaseMode::Assisted);
    assert_eq!(record.state, ReleaseState::Detected);
    assert_eq!(record.transitions.len(), 1);
    assert_eq!(record.transitions[0].from, None);
    assert_eq!(record.transitions[0].at, record.created_at);
    assert_eq!(record.updated_at, record.created_at);
    assert_eq!(record.previous_version.as_deref(), Some("1.0.0"));
    assert_eq!(record.svn_url, "https://plugins.svn.wordpress.org/sample-plugin");
    assert_eq!(record.release_kind, Some(rd_records::ReleaseKind::Version));
    assert_eq!(
        record.commit_marker(),
        format!("ReleaseDock-Release-ID: {}", record.id.to_string().to_uppercase())
    );
}

#[test]
fn transition_at_records_the_date() {
    let mut record = ReleaseRecord::new(&default_product(), ReleaseSource::zip("/x.zip"), ReleaseMode::Assisted);
    let date = rd_model::SwiftDate(800_000_000.0);
    ReleaseStateMachine::transition_at(&mut record, ReleaseState::Inspected, Some("ok"), date).unwrap();
    assert_eq!(record.updated_at, date);
    assert_eq!(record.transitions.last().unwrap().at, date);
    assert_eq!(record.transitions.last().unwrap().note.as_deref(), Some("ok"));
}
