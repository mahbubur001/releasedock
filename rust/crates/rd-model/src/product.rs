use crate::time::{Id, SwiftDate};
use crate::validation::ValidationCheckId;
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeSet;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProductType {
    Plugin,
    Theme,
}

impl ProductType {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Plugin => "Plugin",
            Self::Theme => "Theme",
        }
    }

    /// Default WordPress.org SVN repository for a slug.
    pub fn default_repository_url(self, slug: &str) -> String {
        match self {
            Self::Plugin => format!("https://plugins.svn.wordpress.org/{slug}"),
            Self::Theme => format!("https://themes.svn.wordpress.org/{slug}"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReleaseMode {
    /// Prepare and validate everything without committing.
    DryRun,
    /// Run every check and require a final user confirmation. Default.
    Assisted,
    /// Publish without confirmation, only when every automation precondition holds.
    Automatic,
}

impl ReleaseMode {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::DryRun => "Dry Run",
            Self::Assisted => "Assisted Release",
            Self::Automatic => "Automatic Release",
        }
    }
}

/// Per-product validation configuration. Required checks cannot be disabled. Missing keys
/// decode to their defaults and unknown check identifiers are dropped, as in Swift.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ValidationSettings {
    /// Treat warnings as blocking.
    pub block_on_warnings: bool,
    /// Require a readme changelog entry for the release version (plugins).
    pub require_changelog_entry: bool,
    /// Require the local development Git working tree to be clean.
    pub require_clean_git_tree: bool,
    /// Additional glob patterns excluded from the release candidate.
    pub exclude_patterns: Vec<String>,
    /// Optional checks the user turned off.
    #[serde(deserialize_with = "known_checks")]
    pub disabled_checks: BTreeSet<ValidationCheckId>,
    /// Package size above which a warning is raised, in megabytes.
    #[serde(rename = "packageSizeWarningMB")]
    pub package_size_warning_mb: i64,
}

impl Default for ValidationSettings {
    fn default() -> Self {
        Self {
            block_on_warnings: false,
            require_changelog_entry: true,
            require_clean_git_tree: false,
            exclude_patterns: Vec::new(),
            disabled_checks: BTreeSet::new(),
            package_size_warning_mb: 10,
        }
    }
}

fn known_checks<'de, D: Deserializer<'de>>(deserializer: D) -> Result<BTreeSet<ValidationCheckId>, D::Error> {
    let raw = Vec::<String>::deserialize(deserializer)?;
    Ok(raw
        .into_iter()
        .filter_map(|id| serde_json::from_value(serde_json::Value::String(id)).ok())
        .collect())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseSummary {
    #[serde(rename = "releaseID")]
    pub release_id: Id,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<i64>,
    pub date: SwiftDate,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubReleaseSettings {
    pub enabled: bool,
    /// Prepended to the version for the tag name, e.g. `v` → `v1.2.0`.
    pub tag_prefix: String,
    pub draft: bool,
}

impl Default for GitHubReleaseSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            tag_prefix: "v".into(),
            draft: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Product {
    pub id: Id,
    pub name: String,
    pub slug: String,
    #[serde(rename = "type")]
    pub product_type: ProductType,
    /// Header name expected in the package (`Plugin Name:` / `Theme Name:`).
    pub expected_header_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_development_path: Option<String>,
    #[serde(rename = "svnRepositoryURL")]
    pub svn_repository_url: String,
    pub working_copy_path: String,
    /// Last known released version on SVN.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_successful_release: Option<ReleaseSummary>,
    pub release_mode: ReleaseMode,
    /// Explicit opt-in required in addition to `release_mode == Automatic`.
    pub automation_enabled: bool,
    pub validation: ValidationSettings,
    #[serde(rename = "githubRepositoryURL", default, skip_serializing_if = "Option::is_none")]
    pub github_repository_url: Option<String>,
    /// Main plugin file relative to the package root, when auto-detection is ambiguous.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub main_plugin_file: Option<String>,
    /// Optional local folder mirrored to the plugin SVN `/assets` directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assets_path: Option<String>,
    /// Shell command run on a copy of a folder source before inspection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build_command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github_release: Option<GitHubReleaseSettings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub svn_username: Option<String>,
    /// Commit summary used last for this product (commit-template tokens).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit_message_template: Option<String>,
    pub created_at: SwiftDate,
    pub updated_at: SwiftDate,
}

impl Product {
    /// A new product with the Swift initializer's defaults.
    pub fn new(
        name: &str,
        slug: &str,
        product_type: ProductType,
        svn_repository_url: &str,
        working_copy_path: &str,
    ) -> Self {
        let now = SwiftDate::now();
        Self {
            id: Id::new(),
            name: name.into(),
            slug: slug.into(),
            product_type,
            expected_header_name: name.into(),
            local_development_path: None,
            svn_repository_url: svn_repository_url.into(),
            working_copy_path: working_copy_path.into(),
            current_version: None,
            last_successful_release: None,
            release_mode: ReleaseMode::Assisted,
            automation_enabled: false,
            validation: ValidationSettings::default(),
            github_repository_url: None,
            main_plugin_file: None,
            assets_path: None,
            build_command: None,
            github_release: None,
            svn_username: None,
            commit_message_template: None,
            created_at: now,
            updated_at: now,
        }
    }

    /// The trimmed build command, or None when none is set.
    pub fn effective_build_command(&self) -> Option<&str> {
        self.build_command.as_deref().map(str::trim).filter(|c| !c.is_empty())
    }

    pub fn working_copy_dir(&self) -> PathBuf {
        PathBuf::from(&self.working_copy_path)
    }

    pub fn local_development_dir(&self) -> Option<PathBuf> {
        self.local_development_path.as_ref().map(PathBuf::from)
    }

    /// Automatic publishing is only possible when both the mode and the explicit opt-in are set.
    pub fn is_automatic_publishing_authorized(&self) -> bool {
        self.release_mode == ReleaseMode::Automatic && self.automation_enabled
    }
}
