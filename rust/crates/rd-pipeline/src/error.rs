//! `ReleaseError` from `Release/ReleaseStagingService.swift`, plus the errors Swift rethrows
//! untyped from the services the pipeline calls.

use rd_integrations::BuildError;
use rd_integrations::WordPressOrgError;
use rd_integrations::repository_layout::LayoutError;
use rd_keychain::KeychainError;
use rd_records::{ManifestError, ReleaseStateError};
use rd_store::StoreError;
use rd_support::path_safety::Violation;
use rd_svn::{SvnError, SvnErrorKind};
use rd_versioning::{ProductDetectionError, VersionBumpError};

/// The pipeline's error. The first sixteen variants are Swift's `ReleaseError` cases with
/// their messages verbatim; the rest carry the errors Swift rethrows from other services
/// (displayed with their own message, like `localizedDescription`).
#[derive(Debug, thiserror::Error)]
pub enum ReleaseError {
    #[error("{0}")]
    InvalidState(String),
    #[error("The product no longer exists.")]
    ProductNotFound,
    #[error("The release record was not found.")]
    ReleaseNotFound,
    #[error("{0} required check(s) failed. Resolve them before publishing.")]
    ValidationBlocked(usize),
    #[error("Automatic publishing is not allowed: {}", .0.join("; "))]
    AutomationNotAuthorized(Vec<String>),
    #[error("The working copy changed after the preview was approved. Review the new preview and approve again.")]
    PreviewChanged,
    #[error("The repository changed on the server: {0}. Prepare the release again.")]
    RemoteChanged(String),
    #[error("{0} already exists on the server.")]
    ReleasePathExists(String),
    #[error("The release candidate changed after inspection: {0}")]
    CandidateChanged(String),
    #[error("Working-copy paths changed type and need manual attention: {}", first(.0, 5).join(", "))]
    WorkingCopyObstructed(Vec<String>),
    #[error("SVN credentials are required to publish.")]
    CredentialsRequired,
    #[error("Release data missing: {0}")]
    MissingData(String),
    #[error("The commit did not reach the repository.")]
    NotCommitted,
    #[error(
        "The package differs from the released {tag} in {} file(s) besides the readme ({}{}). Publish a new version to ship code changes.",
        paths.len(),
        first(paths, 5).join(", "),
        if paths.len() > 5 { ", …" } else { "" }
    )]
    ReadmeUpdateChangesCode { tag: String, paths: Vec<String> },
    /// Mapped from `rd_integrations::BuildError::BuildFailed` (same message).
    #[error(
        "The build command failed (exit code {exit_code}): {command}\n{}",
        suffix(output, 800)
    )]
    BuildFailed {
        command: String,
        exit_code: i32,
        output: String,
    },
    #[error("{0}")]
    RestoreNotPossible(String),

    // Errors Swift rethrows from other services.
    #[error(transparent)]
    State(#[from] ReleaseStateError),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Svn(#[from] SvnError),
    #[error(transparent)]
    Inspection(#[from] rd_inspection::Error),
    #[error(transparent)]
    Detection(#[from] ProductDetectionError),
    #[error(transparent)]
    VersionBump(#[from] VersionBumpError),
    /// A build that could not run at all (copy or shell failure); a failing command is
    /// [`ReleaseError::BuildFailed`].
    #[error(transparent)]
    Build(BuildError),
    #[error(transparent)]
    Keychain(#[from] KeychainError),
    #[error(transparent)]
    Manifest(#[from] ManifestError),
    #[error(transparent)]
    Path(#[from] Violation),
    #[error(transparent)]
    WordPressOrg(#[from] WordPressOrgError),
    /// A file system operation failed (Swift rethrows the `FileManager` error).
    #[error("{0}")]
    Io(String),
}

impl ReleaseError {
    /// The svn error kind, when this is a failed svn command.
    pub fn svn_kind(&self) -> Option<SvnErrorKind> {
        match self {
            Self::Svn(error) => error.kind(),
            _ => None,
        }
    }
}

impl From<BuildError> for ReleaseError {
    fn from(error: BuildError) -> Self {
        match error {
            BuildError::BuildFailed {
                command,
                exit_code,
                output,
            } => Self::BuildFailed {
                command,
                exit_code,
                output,
            },
            other => Self::Build(other),
        }
    }
}

impl From<LayoutError> for ReleaseError {
    fn from(error: LayoutError) -> Self {
        match error {
            LayoutError::Svn(error) => Self::Svn(error),
            LayoutError::Io(message) => Self::Io(message),
        }
    }
}

impl From<rd_inspection::PackageInspectionError> for ReleaseError {
    fn from(error: rd_inspection::PackageInspectionError) -> Self {
        Self::Inspection(error.into())
    }
}

/// Wraps an `io::Error` with the path it concerns.
pub(crate) fn io_error(path: &std::path::Path) -> impl FnOnce(std::io::Error) -> ReleaseError + '_ {
    move |error| ReleaseError::Io(format!("{}: {error}", path.display()))
}

fn first(items: &[String], count: usize) -> &[String] {
    &items[..items.len().min(count)]
}

/// Swift `String.suffix(_:)` on characters.
fn suffix(text: &str, count: usize) -> &str {
    let total = text.chars().count();
    if total <= count {
        return text;
    }
    let start = text.char_indices().nth(total - count).map_or(0, |(index, _)| index);
    &text[start..]
}
