//! Repository facts gathered before validation, and the working-copy state shown on the
//! Products page. Ported from `Models/RepositorySnapshot.swift`.

use rd_model::SvnItemStatus;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AuthenticationAvailability {
    /// Credentials stored in ReleaseDock's Keychain item.
    Keychain,
    /// svn has credentials cached for the repository realm.
    #[serde(rename = "cachedBySVN")]
    CachedBySvn,
    /// No credentials anywhere; publishing will need them.
    None,
    #[default]
    Unknown,
}

/// Facts about the SVN repository and working copy gathered before validation.
/// Validation itself is pure and reads only this snapshot.
///
/// `Default` matches the Swift initializer's defaults.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositorySnapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub svn_version: Option<String>,
    /// None when the repository was reachable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connectivity_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository_exists: Option<bool>,
    /// Released version according to the repository (plugin Stable tag / highest theme version).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_version_source: Option<String>,
    pub existing_versions: Vec<String>,
    /// Whether the candidate version's tag/directory already exists remotely.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_path_exists: Option<bool>,
    pub structure_problems: Vec<String>,
    pub working_copy_exists: bool,
    #[serde(rename = "workingCopyURLMismatch", default, skip_serializing_if = "Option::is_none")]
    pub working_copy_url_mismatch: Option<String>,
    pub working_copy_changes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_copy_revision: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_revision: Option<i64>,
    pub out_of_date_paths: Vec<String>,
    pub authentication: AuthenticationAvailability,
    pub notes: Vec<String>,
    /// Title line of the server's trunk readme (plugins), used to name new products.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub readme_title: Option<String>,
    /// Latest stable WordPress release when WordPress.org could be reached.
    #[serde(rename = "latestWordPressVersion", default, skip_serializing_if = "Option::is_none")]
    pub latest_wordpress_version: Option<String>,
}

/// One local change in a product's working copy.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct WorkingCopyChange {
    /// Relative to the working-copy root.
    pub path: String,
    pub status: SvnItemStatus,
    pub is_conflict: bool,
    pub is_tree_conflict: bool,
}

impl WorkingCopyChange {
    pub fn new(path: impl Into<String>, status: SvnItemStatus) -> Self {
        Self {
            path: path.into(),
            status,
            is_conflict: false,
            is_tree_conflict: false,
        }
    }

    pub fn id(&self) -> &str {
        &self.path
    }
}

/// A product's working copy as the Products page shows it.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Default)]
pub struct WorkingCopyState {
    pub exists: bool,
    pub revision: Option<i64>,
    /// Everything that would fail the "Working copy cleanliness" check.
    pub changes: Vec<WorkingCopyChange>,
}

impl WorkingCopyState {
    pub fn new(exists: bool, revision: Option<i64>, changes: Vec<WorkingCopyChange>) -> Self {
        Self {
            exists,
            revision,
            changes,
        }
    }

    pub fn conflicts(&self) -> Vec<&WorkingCopyChange> {
        self.changes.iter().filter(|c| c.is_conflict).collect()
    }

    pub fn is_clean(&self) -> bool {
        self.changes.is_empty()
    }
}

/// Which side wins when resolving a conflict.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConflictResolution {
    /// Take the WordPress.org version; the local change is backed up first.
    Server,
    /// Keep the local version.
    Mine,
}

impl ConflictResolution {
    pub const ALL: [Self; 2] = [Self::Server, Self::Mine];

    pub fn raw_value(self) -> &'static str {
        match self {
            Self::Server => "server",
            Self::Mine => "mine",
        }
    }
}
