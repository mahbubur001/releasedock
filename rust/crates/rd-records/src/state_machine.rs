//! The release lifecycle. Ported from `Release/ReleaseStateMachine.swift`.

use crate::release::{ReleaseRecord, ReleaseState, StateTransition};
use rd_model::{ReleaseMode, SwiftDate};
use rd_support::redact::redact;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ReleaseStateError {
    #[error("Invalid release state transition: {} → {}", from.display_name(), to.display_name())]
    InvalidTransition { from: ReleaseState, to: ReleaseState },
    #[error("A dry run never publishes.")]
    DryRunCannotPublish,
}

/// The release lifecycle:
///
/// ```text
/// Detected → Inspected → Validated → Staged → Previewed → Approved → Publishing → Verifying → Completed
/// ```
///
/// plus Failed / Cancelled (terminal) and RecoveryRequired. Once a commit may have
/// reached the server (`publishing` onward), cancellation is no longer possible:
/// the outcome must be established by inspecting the repository.
pub struct ReleaseStateMachine;

impl ReleaseStateMachine {
    pub fn allowed_targets(state: ReleaseState, mode: ReleaseMode) -> BTreeSet<ReleaseState> {
        use ReleaseState::*;
        let targets: &[ReleaseState] = match state {
            Detected => &[Inspected, Failed, Cancelled],
            Inspected => &[Validated, Failed, Cancelled],
            Validated => &[Staged, Failed, Cancelled],
            Staged => &[Previewed, Failed, Cancelled],
            // A dry run ends after the preview; it can never be approved.
            Previewed if mode == ReleaseMode::DryRun => &[Completed, Failed, Cancelled],
            Previewed => &[Approved, Staged, Failed, Cancelled],
            Approved => &[Publishing, Previewed, Failed, Cancelled],
            // A commit rejected before reaching the repository goes to `failed`, or back to
            // `previewed` when it can be retried after fixing credentials (re-approval needed);
            // an uncertain outcome goes to `recoveryRequired`.
            Publishing => &[Verifying, Failed, RecoveryRequired, Previewed],
            Verifying => &[Completed, RecoveryRequired],
            // After inspecting the repository: the release is there (verify it), or provably
            // absent (fail it, or with explicit approval publish again).
            RecoveryRequired => &[Verifying, Failed, Approved],
            Completed | Failed | Cancelled => &[],
        };
        targets.iter().copied().collect()
    }

    pub fn can_transition(from: ReleaseState, to: ReleaseState, mode: ReleaseMode) -> bool {
        Self::allowed_targets(from, mode).contains(&to)
    }

    /// Applies a transition to the record now, appending to its history.
    pub fn transition(
        record: &mut ReleaseRecord,
        target: ReleaseState,
        note: Option<&str>,
    ) -> Result<(), ReleaseStateError> {
        Self::transition_at(record, target, note, SwiftDate::now())
    }

    /// Applies a transition to the record at `date`, appending to its history. The note is
    /// redacted before it is stored.
    pub fn transition_at(
        record: &mut ReleaseRecord,
        target: ReleaseState,
        note: Option<&str>,
        date: SwiftDate,
    ) -> Result<(), ReleaseStateError> {
        if record.mode == ReleaseMode::DryRun && matches!(target, ReleaseState::Approved | ReleaseState::Publishing) {
            return Err(ReleaseStateError::DryRunCannotPublish);
        }
        if !Self::can_transition(record.state, target, record.mode) {
            return Err(ReleaseStateError::InvalidTransition {
                from: record.state,
                to: target,
            });
        }
        record
            .transitions
            .push(StateTransition::new(Some(record.state), target, date, note.map(redact)));
        record.state = target;
        record.updated_at = date;
        Ok(())
    }

    /// State a non-terminal release should move to after the app restarted mid-release.
    /// Anything that may have committed requires recovery; everything earlier is cancelled
    /// because nothing left the machine.
    pub fn state_after_interruption(state: ReleaseState) -> Option<ReleaseState> {
        if state.is_terminal() || state == ReleaseState::RecoveryRequired {
            return None;
        }
        Some(if state.may_have_committed() {
            ReleaseState::RecoveryRequired
        } else {
            ReleaseState::Cancelled
        })
    }
}
