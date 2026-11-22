import Foundation

public enum ReleaseStateError: Error, LocalizedError, Equatable, Sendable {
    case invalidTransition(from: ReleaseState, to: ReleaseState)
    case dryRunCannotPublish

    public var errorDescription: String? {
        switch self {
        case .invalidTransition(let from, let to): "Invalid release state transition: \(from.displayName) → \(to.displayName)"
        case .dryRunCannotPublish: "A dry run never publishes."
        }
    }
}

/// The release lifecycle:
///
///     Detected → Inspected → Validated → Staged → Previewed → Approved → Publishing → Verifying → Completed
///
/// plus Failed / Cancelled (terminal) and RecoveryRequired. Once a commit may have
/// reached the server (`publishing` onward), cancellation is no longer possible:
/// the outcome must be established by inspecting the repository.
public enum ReleaseStateMachine {
    public static func allowedTargets(from state: ReleaseState, mode: ReleaseMode) -> Set<ReleaseState> {
        switch state {
        case .detected: [.inspected, .failed, .cancelled]
        case .inspected: [.validated, .failed, .cancelled]
        case .validated: [.staged, .failed, .cancelled]
        case .staged: [.previewed, .failed, .cancelled]
        case .previewed:
            // A dry run ends after the preview; it can never be approved.
            mode == .dryRun ? [.completed, .failed, .cancelled] : [.approved, .staged, .failed, .cancelled]
        case .approved: [.publishing, .previewed, .failed, .cancelled]
        // A commit rejected before reaching the repository goes to `failed`, or back to
        // `previewed` when it can be retried after fixing credentials (re-approval needed);
        // an uncertain outcome goes to `recoveryRequired`.
        case .publishing: [.verifying, .failed, .recoveryRequired, .previewed]
        case .verifying: [.completed, .recoveryRequired]
        // After inspecting the repository: the release is there (verify it), or provably
        // absent (fail it, or with explicit approval publish again).
        case .recoveryRequired: [.verifying, .failed, .approved]
        case .completed, .failed, .cancelled: []
        }
    }

    public static func canTransition(from: ReleaseState, to: ReleaseState, mode: ReleaseMode) -> Bool {
        allowedTargets(from: from, mode: mode).contains(to)
    }

    /// Applies a transition to the record, appending to its history.
    public static func transition(_ record: inout ReleaseRecord, to target: ReleaseState, note: String? = nil, at date: Date = Date()) throws(ReleaseStateError) {
        if record.mode == .dryRun, target == .approved || target == .publishing {
            throw .dryRunCannotPublish
        }
        guard canTransition(from: record.state, to: target, mode: record.mode) else {
            throw .invalidTransition(from: record.state, to: target)
        }
        record.transitions.append(StateTransition(from: record.state, to: target, at: date, note: note.map(SecretRedactor.redact)))
        record.state = target
        record.updatedAt = date
    }

    /// State a non-terminal release should move to after the app restarted mid-release.
    /// Anything that may have committed requires recovery; everything earlier is cancelled
    /// because nothing left the machine.
    public static func stateAfterInterruption(_ state: ReleaseState) -> ReleaseState? {
        if state.isTerminal { return nil }
        if state == .recoveryRequired { return nil }
        return state.mayHaveCommitted ? .recoveryRequired : .cancelled
    }
}
