//! The repository part of `Release/PluginReleaseStrategy.swift`:
//!
//! ```text
//! <slug>/
//! ├── assets/   banners, icons, screenshots (optional sync)
//! ├── tags/     one directory per released version
//! └── trunk/    latest code; trunk/readme.txt "Stable tag" selects the served tag
//! ```
//!
//! The release is one atomic commit containing the trunk update and `svn cp trunk
//! tags/<version>` (the approach used by 10up/action-wordpress-plugin-deploy).

use crate::error::ReleaseError;
use crate::fs_util::{file_exists, path_extension, path_string, trimming_trailing_slash};
use crate::strategy::{PluginReleaseStrategy, RepositoryStrategy};
use crate::working_copy::{self, cat_if_exists};
use async_trait::async_trait;
use rd_inspection::{FolderInspecting, FolderInspectionService, InspectionOptions, PackageInspectionError};
use rd_integrations::repository_layout;
use rd_model::Product;
use rd_records::{RepositorySnapshot, StagedCandidate, VerificationReport};
use rd_support::hashing;
use rd_svn::{SvnDepth, SvnNodeKind, SvnServicing};
use rd_versioning::readme;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// MIME types set on directory assets so images display instead of downloading.
pub const ASSET_MIME_TYPES: [(&str, &str); 5] = [
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("gif", "image/gif"),
    ("svg", "image/svg+xml"),
];

const CHECKSUM_NOTE: &str =
    "Checksum differences can come from svn:eol-style or keyword properties applied by the repository.";

/// Mirrors a local assets folder into the working copy's `/assets` and sets MIME types.
pub async fn sync_assets(source: &Path, target: &Path, svn: &dyn SvnServicing) -> Result<(), ReleaseError> {
    let inspection = FolderInspectionService::new().inspect(source, &InspectionOptions::default())?;
    let blocking: Vec<_> = inspection.blocking_issues().into_iter().cloned().collect();
    if !blocking.is_empty() {
        return Err(PackageInspectionError::BlockingIssues(blocking).into());
    }
    let candidate = StagedCandidate::new(
        path_string(source),
        inspection.included_entries.clone(),
        inspection.sha256.clone(),
    );
    working_copy::mirror(&candidate, target, svn).await?;
    // Set MIME types explicitly so images display instead of downloading.
    for (extension, mime) in ASSET_MIME_TYPES {
        let files: Vec<PathBuf> = inspection
            .included_files()
            .into_iter()
            .filter(|f| path_extension(&f.path).to_lowercase() == extension)
            .map(|f| target.join(&f.path))
            .collect();
        svn.set_property("svn:mime-type", mime, &files).await?;
    }
    Ok(())
}

/// The trunk readme's file name on the server, if any.
async fn trunk_readme(base: &str, svn: &dyn SvnServicing) -> Result<Option<Vec<u8>>, ReleaseError> {
    let trunk_files: Vec<String> = svn
        .list(&format!("{base}/trunk"), false)
        .await?
        .into_iter()
        .filter(|e| e.kind == SvnNodeKind::File)
        .map(|e| e.name)
        .collect();
    let Some(name) = PluginReleaseStrategy::readme_name(&trunk_files) else {
        return Ok(None);
    };
    Ok(cat_if_exists(&format!("{base}/trunk/{name}"), svn).await?)
}

#[async_trait]
impl RepositoryStrategy for PluginReleaseStrategy {
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
        _current_version: Option<&str>,
        product: &Product,
        svn: &dyn SvnServicing,
    ) -> Result<(), ReleaseError> {
        let wc = product.working_copy_dir();
        let tag = wc.join("tags").join(version);
        if file_exists(&tag) {
            return Err(ReleaseError::ReleasePathExists(format!("tags/{version}")));
        }
        working_copy::mirror(candidate, &wc.join("trunk"), svn).await?;
        if let Some(assets_path) = &product.assets_path {
            sync_assets(Path::new(assets_path), &wc.join("assets"), svn).await?;
        }
        // Copy trunk (with its scheduled changes) to the new tag; committed atomically together.
        svn.copy(&wc.join("trunk"), &tag).await?;
        Ok(())
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
        let tag_url = format!("{base}/tags/{version}");
        if !svn.exists(&tag_url).await? {
            let mut report = VerificationReport::new(None, false, tag_url);
            report.notes = vec!["The tag does not exist on the server.".into()];
            return Ok(report);
        }
        let info = svn.info(&tag_url).await?;
        let tree = working_copy::verify_remote_tree(&tag_url, candidate, svn, scratch).await?;
        let mut stable_matches = None;
        let mut notes = Vec::new();
        if let Some(data) = trunk_readme(base, svn).await? {
            let stable = readme::parse(&String::from_utf8_lossy(&data)).stable_tag();
            let matches = stable.as_deref() == Some(version);
            stable_matches = Some(matches);
            if !matches {
                notes.push(format!(
                    "trunk readme Stable tag is {}, not {version}.",
                    stable.as_deref().unwrap_or("missing")
                ));
            }
        }
        if !tree.mismatched.is_empty() {
            notes.push(CHECKSUM_NOTE.into());
        }
        Ok(VerificationReport {
            committed_revision: info.last_changed_revision,
            tag_exists: true,
            tag_url,
            missing_files: tree.missing,
            unexpected_files: tree.unexpected,
            checksum_mismatches: tree.mismatched,
            stable_tag_matches: stable_matches,
            notes,
        })
    }

    fn post_release_guidance(&self, product: &Product, version: &str) -> Vec<String> {
        vec![
            format!(
                "WordPress.org holds new releases for a security review period (currently about 6 hours) before offering the update to sites. ReleaseDock checks the directory and notifies you when {version} is live."
            ),
            format!(
                "If Release Confirmation is enabled for {}, every committer receives a \"Pending release\" email. Confirm version {version} at https://wordpress.org/plugins/developers/releases/ — users stay on the previous version until it is confirmed.",
                product.name
            ),
            format!("Do not modify tags/{version} after it is confirmed; changes require confirming again."),
        ]
    }

    // MARK: Readme updates

    async fn apply_readme_update(
        &self,
        candidate: &StagedCandidate,
        readme_file: &str,
        version: &str,
        product: &Product,
        svn: &dyn SvnServicing,
    ) -> Result<(), ReleaseError> {
        let wc = product.working_copy_dir();
        let tag = wc.join("tags").join(version);
        if !file_exists(&tag) {
            return Err(ReleaseError::InvalidState(format!(
                "tags/{version} is not in the working copy."
            )));
        }
        // Tags are checked out empty (sparse); the released one is needed in full to compare.
        svn.update(&tag, Some(SvnDepth::Infinity)).await?;
        let ignoring = HashSet::from([readme_file.to_owned()]);
        let differences = working_copy::differences(candidate, &tag, &ignoring)?;
        if !differences.is_empty() {
            return Err(ReleaseError::ReadmeUpdateChangesCode {
                tag: format!("tags/{version}"),
                paths: differences,
            });
        }

        let readme = candidate.root_dir().join(readme_file);
        working_copy::replace_file(&wc.join("trunk").join(readme_file), &readme, svn).await?;
        working_copy::replace_file(&tag.join(readme_file), &readme, svn).await?;
        if let Some(assets_path) = &product.assets_path {
            sync_assets(Path::new(assets_path), &wc.join("assets"), svn).await?;
        }
        Ok(())
    }

    async fn verify_readme_update(
        &self,
        candidate: &StagedCandidate,
        readme_file: &str,
        version: &str,
        product: &Product,
        svn: &dyn SvnServicing,
    ) -> Result<VerificationReport, ReleaseError> {
        let base = trimming_trailing_slash(&product.svn_repository_url);
        let tag_url = format!("{base}/tags/{version}");
        if !svn.exists(&tag_url).await? {
            let mut report = VerificationReport::new(None, false, tag_url);
            report.notes = vec!["The tag does not exist on the server.".into()];
            return Ok(report);
        }
        let expected = candidate
            .entries
            .iter()
            .find(|e| e.path == readme_file)
            .and_then(|e| e.sha256.clone());
        let mut missing = Vec::new();
        let mut mismatched = Vec::new();
        let mut revision: Option<i64> = None;
        let mut stable_matches = None;
        for path in [format!("trunk/{readme_file}"), format!("tags/{version}/{readme_file}")] {
            let Some(data) = cat_if_exists(&format!("{base}/{path}"), svn).await? else {
                missing.push(path);
                continue;
            };
            if Some(hashing::sha256(&data)) != expected {
                mismatched.push(path.clone());
            }
            if path.starts_with("trunk/") {
                stable_matches =
                    Some(readme::parse(&String::from_utf8_lossy(&data)).stable_tag().as_deref() == Some(version));
            }
            let changed = svn.info(&format!("{base}/{path}")).await?.last_changed_revision;
            revision = Some(revision.unwrap_or(0).max(changed.unwrap_or(0)));
        }
        let mut notes = Vec::new();
        if stable_matches == Some(false) {
            notes.push(format!("trunk readme Stable tag is not {version}."));
        }
        if !mismatched.is_empty() {
            notes.push(CHECKSUM_NOTE.into());
        }
        Ok(VerificationReport {
            committed_revision: revision,
            tag_exists: true,
            tag_url,
            missing_files: missing,
            unexpected_files: Vec::new(),
            checksum_mismatches: mismatched,
            stable_tag_matches: stable_matches,
            notes,
        })
    }

    fn post_readme_update_guidance(&self, product: &Product, version: &str) -> Vec<String> {
        vec![
            "WordPress.org refreshes the plugin page within minutes. Banner, icon and screenshot changes can take up to 6 hours to appear because of caching.".into(),
            format!(
                "If Release Confirmation is enabled for {}, changing tags/{version} may need confirming again at https://wordpress.org/plugins/developers/releases/.",
                product.name
            ),
        ]
    }
}
