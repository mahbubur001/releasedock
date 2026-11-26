//! Read-only inspection of release packages. Ported from `ReleaseDockCore/Inspection`.
//!
//! - [`zip_archive`]: our own defensive ZIP reader (a security control, not a general
//!   purpose library).
//! - [`zip_inspection`] / [`folder_inspection`]: turn a ZIP or a source folder into a
//!   [`rd_model::PackageInspection`] without modifying the source.
//! - [`file_classifier`] / [`path_evaluator`]: junk, VCS, development, forbidden and secret
//!   files, `.distignore` / `export-ignore` / user glob patterns.
//! - [`asset_inspector`]: WordPress.org directory assets (`/assets` in SVN).

pub mod asset_inspector;
pub mod error;
pub mod file_classifier;
pub mod folder_inspection;
pub mod path_evaluator;
mod text;
pub mod zip_archive;
pub mod zip_inspection;

pub use asset_inspector::{AssetFile, AssetInspector, AssetsReport, asset_rules};
pub use error::{Error, PackageInspectionError};
pub use file_classifier::{FileClassification, FileClassifier, GlobPattern};
pub use folder_inspection::{FolderInspecting, FolderInspectionService};
pub use path_evaluator::{InspectionOptions, PackagePathEvaluator, issue_kind};
pub use zip_archive::{ZipArchive, ZipEntry, ZipError};
pub use zip_inspection::{ZipInspecting, ZipInspectionService};
