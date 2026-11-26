//! Product-type specific release rules. Ported from the pure part of the `ReleaseStrategy`
//! protocol and `StrategyProvider` in `Release/ReleaseStrategy.swift`.
//!
//! The Swift protocol also inspects the remote repository, prepares and applies the working
//! copy and verifies the commit. Those methods need the SVN client and are ported with the
//! release pipeline, as a trait the pipeline implements for [`PluginReleaseStrategy`] and
//! [`ThemeReleaseStrategy`].

use rd_model::{PackageInspection, Product, ProductType, ValidationResult};
use rd_versioning::DetectedProduct;

pub use crate::plugin_strategy::PluginReleaseStrategy;
pub use crate::theme_strategy::ThemeReleaseStrategy;

/// Product-type specific release behaviour. Plugins and themes use different SVN
/// layouts and rules, so nothing here is shared implicitly between them.
pub trait ReleaseStrategy: Send + Sync {
    fn product_type(&self) -> ProductType;

    /// The directory a release creates, relative to the working-copy root
    /// (`tags/1.2.0` for plugins, `1.2.0` for themes).
    fn release_path(&self, version: &str) -> String;
    /// Working-copy-relative prefixes a release may change.
    fn allowed_change_prefixes(&self, version: &str) -> Vec<String>;

    // Validation (pure)
    fn validate_metadata(&self, detected: &DetectedProduct, product: &Product) -> Vec<ValidationResult>;
    fn validate_required_files(
        &self,
        inspection: &PackageInspection,
        detected: &DetectedProduct,
    ) -> Vec<ValidationResult>;
    fn validate_forbidden_files(&self, inspection: &PackageInspection) -> Vec<ValidationResult>;
    fn validate_version_format(&self, version: &str) -> Vec<ValidationResult>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct StrategyProvider;

impl StrategyProvider {
    pub fn new() -> Self {
        Self
    }

    pub fn strategy(&self, product_type: ProductType) -> &'static dyn ReleaseStrategy {
        match product_type {
            ProductType::Plugin => &PluginReleaseStrategy,
            ProductType::Theme => &ThemeReleaseStrategy,
        }
    }
}
