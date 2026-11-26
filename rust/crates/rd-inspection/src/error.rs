//! Errors of this crate. Swift throws several unrelated error types from the inspection
//! services; Rust collects them in [`Error`].

use crate::zip_archive::ZipError;
use rd_model::InspectionIssue;
use rd_support::path_safety::Violation;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PackageInspectionError {
    #[error("The package has {} blocking issue(s): {}", .0.len(), describe(.0))]
    BlockingIssues(Vec<InspectionIssue>),
    #[error("The source changed after it was inspected: {0}")]
    SourceChanged(String),
    #[error("Destination is not empty: {0}")]
    DestinationNotEmpty(String),
}

fn describe(issues: &[InspectionIssue]) -> String {
    issues
        .iter()
        .take(3)
        .map(|i| format!("{} {}", i.path.as_deref().unwrap_or(""), i.message))
        .collect::<Vec<_>>()
        .join("; ")
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Zip(#[from] ZipError),
    #[error(transparent)]
    Package(#[from] PackageInspectionError),
    #[error(transparent)]
    Path(#[from] Violation),
    /// `ProductDetectionError.notADirectory` in Swift.
    #[error("Not a directory: {0}")]
    NotADirectory(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
