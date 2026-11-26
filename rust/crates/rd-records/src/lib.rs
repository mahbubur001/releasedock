//! Release records and the pure release logic. Ported from `ReleaseDockCore`:
//! `Models/Release.swift`, `Models/RepositorySnapshot.swift`, `Release/ReleaseStateMachine.swift`,
//! `Release/AutomationGate.swift`, `Release/ReleaseManifest.swift` and the stored data types of
//! the code check, Git, GitHub, build, staging and WordPress.org services.
//!
//! Every stored type reads and writes the Swift app's release JSON unchanged (see
//! `tests/compatibility.rs`). Types that already live in other crates are re-exported, not
//! duplicated.

pub mod automation_gate;
pub mod build;
pub mod code_checks;
pub mod git;
pub mod github;
pub mod manifest;
pub mod release;
pub mod repository_snapshot;
pub mod staging;
pub mod state_machine;
pub mod wordpress_org;

pub use automation_gate::AutomationGate;
pub use build::BuildResult;
pub use code_checks::{CodeCheckFinding, CodeCheckReport, CodeCheckTool};
pub use git::GitRepositoryState;
pub use github::{GitHubReleaseResult, GitHubReleaseSettings};
pub use manifest::{ManifestError, ManifestFile, ReleaseManifest};
pub use release::{
    Approval, ApprovalKind, ChangeKind, DIRECTORY_WAIT_WINDOW, DirectoryState, DirectoryStatus, PreviewChange,
    ReleaseKind, ReleasePreview, ReleaseRecord, ReleaseSource, ReleaseState, RestoreOptions, StateTransition,
    VerificationReport,
};
pub use repository_snapshot::{
    AuthenticationAvailability, ConflictResolution, RepositorySnapshot, WorkingCopyChange, WorkingCopyState,
};
pub use staging::StagedCandidate;
pub use state_machine::{ReleaseStateError, ReleaseStateMachine};
pub use wordpress_org::DirectoryListing;

// Types stored in a release record that live in other crates.
pub use rd_inspection::{AssetFile, AssetsReport};
pub use rd_model::SvnItemStatus;
pub use rd_model::{
    EntryKind, Id, InspectionIssue, InspectionIssueKind, PackageEntry, PackageInspection, SourceKind, SwiftDate,
    ValidationCheckId, ValidationReport, ValidationResult, ValidationSeverity, ValidationStatus,
};
pub use rd_versioning::{DetectedProduct, PixelSize, WordPressReadme};
