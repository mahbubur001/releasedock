//! The validation input. Ported from `ValidationContext` in `Validation/ValidationService.swift`.

use rd_inspection::AssetsReport;
use rd_model::{PackageInspection, Product};
use rd_records::{CodeCheckReport, GitRepositoryState, ReleaseKind, ReleasePreview, RepositorySnapshot};
use rd_versioning::DetectedProduct;

/// Everything validation looks at. Gathering (filesystem, SVN, Git) happens before;
/// validation itself is a pure function of this context, which keeps it testable.
#[derive(Clone, Debug, PartialEq)]
pub struct ValidationContext {
    pub product: Product,
    pub inspection: PackageInspection,
    pub detected: Option<DetectedProduct>,
    pub detection_error: Option<String>,
    pub repository: Option<RepositorySnapshot>,
    pub git: Option<GitRepositoryState>,
    /// Present once the release has been staged and previewed.
    pub preview: Option<ReleasePreview>,
    pub kind: ReleaseKind,
    /// Present once the staged candidate has been linted / checked.
    pub code_checks: Option<CodeCheckReport>,
    /// The plugin's assets folder, when one is configured.
    pub assets: Option<AssetsReport>,
    /// Set when the package is an older release's code exported from SVN.
    pub restores_version: Option<String>,
}

impl ValidationContext {
    /// The Swift initializer with its defaults: no detection error, Git state, preview, code
    /// checks, assets or restored version, and a [`ReleaseKind::Version`] release.
    pub fn new(
        product: Product,
        inspection: PackageInspection,
        detected: Option<DetectedProduct>,
        repository: Option<RepositorySnapshot>,
    ) -> Self {
        Self {
            product,
            inspection,
            detected,
            detection_error: None,
            repository,
            git: None,
            preview: None,
            kind: ReleaseKind::Version,
            code_checks: None,
            assets: None,
            restores_version: None,
        }
    }
}
