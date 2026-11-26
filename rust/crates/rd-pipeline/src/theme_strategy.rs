//! The repository part of `Release/ThemeReleaseStrategy.swift`. Themes have no trunk/tags:
//! every version is its own top-level directory (`<slug>/<version>/`). A new version is
//! created by copying the previous version directory (`svn cp 1.0.1 1.0.2`), replacing its
//! contents, and committing exactly one new version directory.
//!
//! WordPress.org imports the commit *afterwards* and runs Theme Check; a failure is emailed
//! to the author and that version never goes live, while the directory stays in SVN and
//! cannot be overwritten. Validation is therefore deliberately strict.

use crate::error::ReleaseError;
use crate::fs_util::{file_exists, trimming_trailing_slash};
use crate::strategy::{RepositoryStrategy, ThemeReleaseStrategy};
use crate::working_copy;
use async_trait::async_trait;
use rd_integrations::repository_layout;
use rd_model::Product;
use rd_records::{RepositorySnapshot, StagedCandidate, VerificationReport};
use rd_svn::SvnServicing;
use std::path::Path;

#[async_trait]
impl RepositoryStrategy for ThemeReleaseStrategy {
    async fn inspect_remote(
        &self,
        product: &Product,
        candidate_version: Option<&str>,
        svn: &dyn SvnServicing,
    ) -> Result<RepositorySnapshot, ReleaseError> {
        Ok(repository_layout::inspect_remote(product, candidate_version, svn).await?)
    }

    async fn prepare_working_copy(
        &self,
        product: &Product,
        current_version: Option<&str>,
        svn: &dyn SvnServicing,
    ) -> Result<(), ReleaseError> {
        Ok(repository_layout::prepare_working_copy(product, current_version, svn).await?)
    }

    async fn apply(
        &self,
        candidate: &StagedCandidate,
        version: &str,
        current_version: Option<&str>,
        product: &Product,
        svn: &dyn SvnServicing,
    ) -> Result<(), ReleaseError> {
        let wc = product.working_copy_dir();
        let target = wc.join(version);
        if file_exists(&target) {
            return Err(ReleaseError::ReleasePathExists(version.to_owned()));
        }
        let Some(current_version) = current_version else {
            return Err(ReleaseError::InvalidState(
                "A theme release needs an existing version directory to copy from.".into(),
            ));
        };
        // Copy the previous version with history, then replace its contents.
        svn.copy(&wc.join(current_version), &target).await?;
        working_copy::mirror(candidate, &target, svn).await
    }

    async fn verify(
        &self,
        candidate: &StagedCandidate,
        version: &str,
        product: &Product,
        svn: &dyn SvnServicing,
        scratch: &Path,
    ) -> Result<VerificationReport, ReleaseError> {
        let base = trimming_trailing_slash(&product.svn_repository_url);
        let url = format!("{base}/{version}");
        if !svn.exists(&url).await? {
            let mut report = VerificationReport::new(None, false, url);
            report.notes = vec!["The version directory does not exist on the server.".into()];
            return Ok(report);
        }
        let info = svn.info(&url).await?;
        let tree = working_copy::verify_remote_tree(&url, candidate, svn, scratch).await?;
        Ok(VerificationReport {
            committed_revision: info.last_changed_revision,
            tag_exists: true,
            tag_url: url,
            missing_files: tree.missing,
            unexpected_files: tree.unexpected,
            checksum_mismatches: tree.mismatched,
            stable_tag_matches: None,
            notes: vec!["WordPress.org imports the version after the commit and runs Theme Check. Watch for a \"Theme Import Failure\" email; a failed version does not go live.".into()],
        })
    }

    fn post_release_guidance(&self, _product: &Product, version: &str) -> Vec<String> {
        vec![
            format!(
                "WordPress.org now imports {version} and runs Theme Check. Success is confirmed by email; a failure arrives as \"Theme Import Failure\" and the version does not go live."
            ),
            "A committed version directory cannot be overwritten. Fixes require a new, higher version.".into(),
            format!(
                "After the import, WordPress.org holds the release for a security review period (currently about 6 hours). ReleaseDock checks the directory and notifies you when {version} is live."
            ),
        ]
    }
}
