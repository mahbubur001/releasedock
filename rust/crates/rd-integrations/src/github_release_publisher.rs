//! Port of `GitHub/GitHubReleasePublisher.swift`. `GitHubReleaseSettings` lives in `rd-model`
//! and `GitHubReleaseResult` in `rd-records`.

use crate::github::{CreateRelease, GitHubError, GitHubReleasing, GitHubRepository};
use rd_model::{Product, SwiftDate, ValidationCheckId, ValidationStatus};
use rd_records::{GitHubReleaseResult, GitHubReleaseSettings, ReleaseRecord};
use rd_versioning::{VersionService, VersionServicing, VersionValidity};
use std::path::Path;
use std::sync::Arc;

/// Creates the GitHub release for a completed WordPress.org release. The tag is created by
/// GitHub on the released commit (`target_commitish`), so the local repository is never
/// written to. Re-running reuses the release for the tag instead of creating a second one.
#[derive(Clone)]
pub struct GitHubReleasePublisher {
    github: Arc<dyn GitHubReleasing>,
}

impl GitHubReleasePublisher {
    pub fn new(github: Arc<dyn GitHubReleasing>) -> Self {
        Self { github }
    }

    pub async fn publish(
        &self,
        record: &ReleaseRecord,
        product: &Product,
        zip: Option<&Path>,
        token: Option<&str>,
    ) -> GitHubReleaseResult {
        let settings = product.github_release.clone().unwrap_or_default();
        let version = record.version.clone().unwrap_or_default();
        let tag = format!("{}{version}", settings.tag_prefix);
        let failure = |message: String| {
            let mut result = GitHubReleaseResult::new(tag.clone(), SwiftDate::now());
            result.error = Some(message);
            result
        };

        let Some(repository) = product
            .github_repository_url
            .as_deref()
            .and_then(GitHubRepository::parse)
        else {
            return failure("Set the product's GitHub repository first.".into());
        };
        let Some(token) = token.filter(|t| !t.is_empty()) else {
            return failure(
                "Add a GitHub token for releases in Settings (fine-grained, Contents: read and write).".into(),
            );
        };
        let Some(commit) = record.git_commit.as_deref() else {
            return failure(
                "GitHub releases need the released commit: release from the Git development folder, not a ZIP.".into(),
            );
        };
        let mut notes = Vec::new();
        let dirty_tree = record.validation.as_ref().is_some_and(|validation| {
            validation
                .results
                .iter()
                .any(|r| r.check == ValidationCheckId::GitWorkingTree && r.status != ValidationStatus::Passed)
        });
        if dirty_tree {
            let short: String = commit.chars().take(10).collect();
            notes.push(format!(
                "The development folder had uncommitted changes when {version} was released; the tag points at {short}."
            ));
        }
        let asset_name = format!("{}-{version}.zip", product.slug);
        match self
            .publish_release(
                &repository,
                &tag,
                commit,
                &version,
                &asset_name,
                &settings,
                record,
                zip,
                token,
                &mut notes,
            )
            .await
        {
            Ok(result) => result,
            Err(error) => {
                let mut result = failure(error.to_string());
                result.notes = notes;
                result
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn publish_release(
        &self,
        repository: &GitHubRepository,
        tag: &str,
        commit: &str,
        version: &str,
        asset_name: &str,
        settings: &GitHubReleaseSettings,
        record: &ReleaseRecord,
        zip: Option<&Path>,
        token: &str,
        notes: &mut Vec<String>,
    ) -> Result<GitHubReleaseResult, GitHubError> {
        if let Some(existing) = self.github.release(tag, repository, token).await? {
            let mut uploaded = None;
            if let Some(zip) = zip {
                if !existing.assets.iter().any(|a| a.name == asset_name) {
                    self.github.upload_asset(zip, asset_name, &existing, token).await?;
                    uploaded = Some(asset_name.to_owned());
                }
            }
            let mut result = GitHubReleaseResult::new(tag, SwiftDate::now());
            result.url = Some(existing.html_url.clone());
            result.asset_name = uploaded.or_else(|| {
                existing
                    .assets
                    .iter()
                    .find(|a| a.name == asset_name)
                    .map(|a| a.name.clone())
            });
            result.already_existed = true;
            result.notes = notes.clone();
            return Ok(result);
        }
        if !self.github.commit_exists(commit, repository, token).await? {
            return Err(GitHubError::CommitNotOnGitHub(commit.to_owned()));
        }
        let is_prerelease = matches!(
            VersionService::new().validate(version),
            VersionValidity::Valid { is_prerelease: true }
        );
        let body = Self::body(record, version);
        let release = self
            .github
            .create_release(
                repository,
                &CreateRelease {
                    tag,
                    commit,
                    name: version,
                    body: &body,
                    draft: settings.draft,
                    prerelease: is_prerelease,
                },
                token,
            )
            .await?;
        if let Some(zip) = zip {
            self.github.upload_asset(zip, asset_name, &release, token).await?;
        } else {
            notes.push("The release ZIP was no longer available, so no asset was attached.".into());
        }
        let mut result = GitHubReleaseResult::new(tag, SwiftDate::now());
        result.url = Some(release.html_url);
        result.asset_name = zip.map(|_| asset_name.to_owned());
        result.notes = notes.clone();
        Ok(result)
    }

    /// The release notes: the readme changelog entry for `version`, then a line saying where
    /// it was published.
    pub fn body(record: &ReleaseRecord, version: &str) -> String {
        let mut parts = Vec::new();
        if let Some(entry) = record
            .detected
            .as_ref()
            .and_then(|d| d.readme.as_ref())
            .and_then(|r| r.changelog_entry(version))
        {
            parts.push(entry);
        }
        let revision = record
            .committed_revision
            .map(|r| format!(" in SVN revision r{r}"))
            .unwrap_or_default();
        parts.push(format!("Published to WordPress.org{revision} with ReleaseDock."));
        parts.join("\n\n")
    }
}
