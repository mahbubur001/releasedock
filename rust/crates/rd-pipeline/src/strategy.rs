//! The SVN side of the `ReleaseStrategy` protocol and `StrategyProvider` from
//! `Release/ReleaseStrategy.swift`. The pure part of the protocol (release paths and
//! validation rules) is `rd_validation::ReleaseStrategy`; [`RepositoryStrategy`] extends it
//! with the methods that need the SVN client, implemented for `rd_validation`'s
//! `PluginReleaseStrategy` and `ThemeReleaseStrategy` in [`crate::plugin_strategy`] and
//! [`crate::theme_strategy`].

use crate::error::ReleaseError;
use async_trait::async_trait;
use rd_model::{Product, ProductType};
use rd_records::{RepositorySnapshot, StagedCandidate, VerificationReport};
use rd_svn::SvnServicing;
use std::path::Path;

pub use rd_validation::{PluginReleaseStrategy, ReleaseStrategy, ThemeReleaseStrategy};

const README_UPDATES_ARE_FOR_PLUGINS: &str = "Readme updates without a new version are only available for plugins.";

/// Product-type specific repository behaviour: inspecting the remote, preparing and
/// changing the working copy, and verifying what was committed.
#[async_trait]
pub trait RepositoryStrategy: ReleaseStrategy {
    /// Reads remote state: structure, released version, existing versions, and whether
    /// `candidate_version`'s release path exists.
    async fn inspect_remote(
        &self,
        product: &Product,
        candidate_version: Option<&str>,
        svn: &dyn SvnServicing,
    ) -> Result<RepositorySnapshot, ReleaseError>;
    /// Checks out (sparsely) or updates the working copy so it is ready for staging.
    /// `current_version` is the latest released version (themes copy from it).
    async fn prepare_working_copy(
        &self,
        product: &Product,
        current_version: Option<&str>,
        svn: &dyn SvnServicing,
    ) -> Result<(), ReleaseError>;
    /// Applies the clean candidate to the working copy and schedules the release copy.
    async fn apply(
        &self,
        candidate: &StagedCandidate,
        version: &str,
        current_version: Option<&str>,
        product: &Product,
        svn: &dyn SvnServicing,
    ) -> Result<(), ReleaseError>;
    /// Verifies the committed release on the server.
    async fn verify(
        &self,
        candidate: &StagedCandidate,
        version: &str,
        product: &Product,
        svn: &dyn SvnServicing,
        scratch: &Path,
    ) -> Result<VerificationReport, ReleaseError>;
    /// Message lines shown after a successful release (e.g. Release Confirmation guidance).
    fn post_release_guidance(&self, product: &Product, version: &str) -> Vec<String>;

    // Readme updates (no new version)

    /// Writes the candidate's readme into trunk and the released version's tag, and syncs
    /// assets. Fails with [`ReleaseError::ReadmeUpdateChangesCode`] when anything else in
    /// the candidate differs from the released tag.
    async fn apply_readme_update(
        &self,
        _candidate: &StagedCandidate,
        _readme_file: &str,
        _version: &str,
        _product: &Product,
        _svn: &dyn SvnServicing,
    ) -> Result<(), ReleaseError> {
        Err(ReleaseError::InvalidState(README_UPDATES_ARE_FOR_PLUGINS.into()))
    }

    /// Verifies that the server's trunk and tag readmes match the candidate's.
    async fn verify_readme_update(
        &self,
        _candidate: &StagedCandidate,
        _readme_file: &str,
        _version: &str,
        _product: &Product,
        _svn: &dyn SvnServicing,
    ) -> Result<VerificationReport, ReleaseError> {
        Err(ReleaseError::InvalidState(README_UPDATES_ARE_FOR_PLUGINS.into()))
    }

    fn post_readme_update_guidance(&self, _product: &Product, _version: &str) -> Vec<String> {
        Vec::new()
    }
}

/// Swift's `StrategyProvider`, returning the strategies with their repository behaviour.
/// (`rd_validation::StrategyProvider` returns the same types for validation only.)
#[derive(Clone, Copy, Debug, Default)]
pub struct StrategyProvider;

impl StrategyProvider {
    pub fn new() -> Self {
        Self
    }

    pub fn strategy(&self, product_type: ProductType) -> &'static dyn RepositoryStrategy {
        match product_type {
            ProductType::Plugin => &PluginReleaseStrategy,
            ProductType::Theme => &ThemeReleaseStrategy,
        }
    }
}
