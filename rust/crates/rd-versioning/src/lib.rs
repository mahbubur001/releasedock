//! Version handling for WordPress plugins and themes. Ported from `ReleaseDockCore/Versioning`:
//! PHP `version_compare()`, file headers, `readme.txt`, product detection, version rules and
//! the version bump of a development folder.

pub mod detector;
pub mod headers;
pub mod php_version;
pub mod readme;
mod text;
pub mod version_bump;
pub mod version_service;

pub use detector::{DetectedProduct, PixelSize, ProductDetectionError, ProductDetector};
pub use readme::WordPressReadme;
pub use version_bump::{Part, VersionBumpChange, VersionBumpError, VersionBumpPlan, VersionBumpService};
pub use version_service::{VersionComparison, VersionService, VersionServicing, VersionValidity};
