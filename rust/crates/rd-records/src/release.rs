//! The release record and everything stored in it. Ported from `Models/Release.swift`.

use crate::build::BuildResult;
use crate::code_checks::CodeCheckReport;
use crate::github::GitHubReleaseResult;
use crate::repository_snapshot::RepositorySnapshot;
use crate::staging::StagedCandidate;
use crate::wordpress_org::DirectoryListing;
use rd_inspection::AssetsReport;
use rd_model::{Id, PackageInspection, Product, ProductType, ReleaseMode, SourceKind, SwiftDate, ValidationReport};
use rd_versioning::{DetectedProduct, Part, VersionBumpService, VersionService, VersionServicing, php_version};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReleaseState {
    Detected,
    Inspected,
    Validated,
    Staged,
    Previewed,
    Approved,
    Publishing,
    Verifying,
    Completed,
    Failed,
    Cancelled,
    RecoveryRequired,
}

impl ReleaseState {
    pub const ALL: [Self; 12] = [
        Self::Detected,
        Self::Inspected,
        Self::Validated,
        Self::Staged,
        Self::Previewed,
        Self::Approved,
        Self::Publishing,
        Self::Verifying,
        Self::Completed,
        Self::Failed,
        Self::Cancelled,
        Self::RecoveryRequired,
    ];

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }

    /// States in which a commit may have reached the server.
    pub fn may_have_committed(self) -> bool {
        matches!(self, Self::Publishing | Self::Verifying | Self::RecoveryRequired)
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Detected => "Detected",
            Self::Inspected => "Inspected",
            Self::Validated => "Validated",
            Self::Staged => "Staged",
            Self::Previewed => "Previewed",
            Self::Approved => "Approved",
            Self::Publishing => "Publishing",
            Self::Verifying => "Verifying",
            Self::Completed => "Completed",
            Self::Failed => "Failed",
            Self::Cancelled => "Cancelled",
            Self::RecoveryRequired => "Recovery Required",
        }
    }
}

/// Where the release package comes from. Encoded like Swift's synthesized `Codable` for an
/// enum with labelled associated values: `{"zip":{"path":"/x.zip"}}`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReleaseSource {
    Zip { path: String },
    Folder { path: String },
}

impl ReleaseSource {
    pub fn zip(path: impl Into<String>) -> Self {
        Self::Zip { path: path.into() }
    }

    pub fn folder(path: impl Into<String>) -> Self {
        Self::Folder { path: path.into() }
    }

    pub fn path(&self) -> &str {
        match self {
            Self::Zip { path } | Self::Folder { path } => path,
        }
    }

    pub fn kind(&self) -> SourceKind {
        match self {
            Self::Zip { .. } => SourceKind::Zip,
            Self::Folder { .. } => SourceKind::Folder,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StateTransition {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<ReleaseState>,
    pub to: ReleaseState,
    pub at: SwiftDate,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl StateTransition {
    pub fn new(from: Option<ReleaseState>, to: ReleaseState, at: SwiftDate, note: Option<String>) -> Self {
        Self { from, to, at, note }
    }
}

/// What a release publishes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReleaseKind {
    /// A new version: trunk is updated and a new tag (or theme version directory) is created.
    Version,
    /// Plugins only: the readme (and `/assets`) of the released version are updated in trunk
    /// and in its tag without a new version, e.g. to raise "Tested up to". The code must be
    /// identical to the released tag.
    ReadmeUpdate,
}

impl ReleaseKind {
    pub const ALL: [Self; 2] = [Self::Version, Self::ReadmeUpdate];

    /// The raw value, used as identity.
    pub fn id(self) -> &'static str {
        match self {
            Self::Version => "version",
            Self::ReadmeUpdate => "readmeUpdate",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Version => "New Version",
            Self::ReadmeUpdate => "Readme & Assets Update",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ApprovalKind {
    User,
    Automatic,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Approval {
    pub kind: ApprovalKind,
    pub at: SwiftDate,
    /// Preview fingerprint the approval applies to. Publishing is refused if it changes.
    pub preview_fingerprint: String,
}

impl Approval {
    pub fn new(kind: ApprovalKind, at: SwiftDate, preview_fingerprint: impl Into<String>) -> Self {
        Self {
            kind,
            at,
            preview_fingerprint: preview_fingerprint.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
    Replaced,
    PropertyChanged,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewChange {
    /// Path relative to the SVN working-copy root (e.g. `trunk/readme.txt`, `tags/1.2.0`).
    pub path: String,
    pub kind: ChangeKind,
    pub is_directory: bool,
    /// True for the tag (or theme version directory) created by copy.
    pub is_copy: bool,
}

impl PreviewChange {
    /// `is_copy` defaults to false in Swift; use [`PreviewChange::copy`] for the tag copy.
    pub fn new(path: impl Into<String>, kind: ChangeKind, is_directory: bool) -> Self {
        Self {
            path: path.into(),
            kind,
            is_directory,
            is_copy: false,
        }
    }

    pub fn copy(path: impl Into<String>, kind: ChangeKind, is_directory: bool) -> Self {
        Self {
            is_copy: true,
            ..Self::new(path, kind, is_directory)
        }
    }

    pub fn id(&self) -> &str {
        &self.path
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleasePreview {
    pub changes: Vec<PreviewChange>,
    /// Unified diff text of modified text files, truncated for display.
    pub diff_excerpt: String,
    pub diff_truncated: bool,
    pub commit_message: String,
    /// Digest over the change list and staged file hashes; approvals bind to it.
    pub fingerprint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_copy_revision: Option<i64>,
    pub created_at: SwiftDate,
}

impl ReleasePreview {
    pub fn added(&self) -> usize {
        self.changes
            .iter()
            .filter(|c| c.kind == ChangeKind::Added && !c.is_directory)
            .count()
    }

    pub fn modified(&self) -> usize {
        self.changes
            .iter()
            .filter(|c| matches!(c.kind, ChangeKind::Modified | ChangeKind::Replaced))
            .count()
    }

    pub fn deleted(&self) -> usize {
        self.changes.iter().filter(|c| c.kind == ChangeKind::Deleted).count()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationReport {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub committed_revision: Option<i64>,
    pub tag_exists: bool,
    #[serde(rename = "tagURL")]
    pub tag_url: String,
    pub missing_files: Vec<String>,
    pub unexpected_files: Vec<String>,
    pub checksum_mismatches: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stable_tag_matches: Option<bool>,
    pub notes: Vec<String>,
}

impl VerificationReport {
    /// A report with the Swift initializer's defaults (empty lists, unknown stable tag).
    pub fn new(committed_revision: Option<i64>, tag_exists: bool, tag_url: impl Into<String>) -> Self {
        Self {
            committed_revision,
            tag_exists,
            tag_url: tag_url.into(),
            missing_files: Vec::new(),
            unexpected_files: Vec::new(),
            checksum_mismatches: Vec::new(),
            stable_tag_matches: None,
            notes: Vec::new(),
        }
    }

    pub fn is_clean(&self) -> bool {
        self.tag_exists
            && self.missing_files.is_empty()
            && self.unexpected_files.is_empty()
            && self.checksum_mismatches.is_empty()
            && self.stable_tag_matches != Some(false)
    }
}

/// `DirectoryStatus.State` in Swift.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DirectoryState {
    /// The directory still offers an older version.
    Pending,
    /// The directory offers this release.
    Live,
    /// The directory already offers a newer version.
    Superseded,
    /// The directory has no listing for the slug.
    NotListed,
}

/// Whether WordPress.org offers a committed release to sites yet. A commit is not
/// distributed immediately: the directory holds releases for a security cooldown,
/// and Release Confirmation (when enabled) waits for a committer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryStatus {
    pub state: DirectoryState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub listed_version: Option<String>,
    pub checked_at: SwiftDate,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub live_since: Option<SwiftDate>,
}

impl DirectoryStatus {
    pub fn new(
        state: DirectoryState,
        listed_version: Option<String>,
        checked_at: SwiftDate,
        live_since: Option<SwiftDate>,
    ) -> Self {
        Self {
            state,
            listed_version,
            checked_at,
            live_since,
        }
    }

    pub fn state(released: &str, listing: Option<&DirectoryListing>) -> DirectoryState {
        let Some(listing) = listing else {
            return DirectoryState::NotListed;
        };
        let Some(listed) = listing.version.as_deref() else {
            return DirectoryState::Pending;
        };
        match php_version::compare(listed, released) {
            0 => DirectoryState::Live,
            order if order > 0 => DirectoryState::Superseded,
            _ => DirectoryState::Pending,
        }
    }
}

/// Persisted record of one release attempt. Everything needed to resume, recover or
/// report the release lives here; credentials never do.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseRecord {
    pub id: Id,
    #[serde(rename = "productID")]
    pub product_id: Id,
    pub product_name: String,
    pub product_slug: String,
    pub product_type: ProductType,
    pub source: ReleaseSource,
    pub mode: ReleaseMode,
    /// None in records created before readme updates existed; read through [`ReleaseRecord::kind`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_kind: Option<ReleaseKind>,
    pub state: ReleaseState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_version: Option<String>,
    #[serde(rename = "sourceSHA256", default, skip_serializing_if = "Option::is_none")]
    pub source_sha256: Option<String>,
    #[serde(rename = "svnURL")]
    pub svn_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inspection: Option<PackageInspection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detected: Option<DetectedProduct>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validation: Option<ValidationReport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<ReleasePreview>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval: Option<Approval>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision_before: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub committed_revision: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification: Option<VerificationReport>,
    pub transitions: Vec<StateTransition>,
    pub errors: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub staging_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate: Option<StagedCandidate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<RepositorySnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_commit: Option<String>,
    /// The release changed the SVN working copy (staged files, scheduled copy).
    pub working_copy_modified: bool,
    /// Recovery inspected the repository and proved the commit did not land.
    pub verified_not_committed: bool,
    /// Post-release instructions for the user (e.g. Release Confirmation).
    pub guidance: Vec<String>,
    /// The exact published package, kept when the product creates GitHub releases.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_zip_path: Option<String>,
    /// Outcome of the GitHub release, when the product creates them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github_release: Option<GitHubReleaseResult>,
    /// The build command's result when the product has one and the source is a folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<BuildResult>,
    /// The plugin's directory-assets folder as inspected for this release.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assets: Option<AssetsReport>,
    /// PHP lint and Plugin Check results for the staged candidate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code_checks: Option<CodeCheckReport>,
    /// Last WordPress.org directory check after the commit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub directory_status: Option<DirectoryStatus>,
    /// Set when this release restores an older released version's code under a new version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restores_version: Option<String>,
    /// Commit summary chosen for this release, with commit-template tokens; None = default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit_template: Option<String>,
    pub created_at: SwiftDate,
    pub updated_at: SwiftDate,
}

/// Default time window in which [`ReleaseRecord::awaits_directory`] keeps checking: 72 hours.
pub const DIRECTORY_WAIT_WINDOW: f64 = 72.0 * 3600.0;

impl ReleaseRecord {
    /// A new version release with the Swift initializer's defaults: a fresh id, kind
    /// `version`, created now.
    pub fn new(product: &Product, source: ReleaseSource, mode: ReleaseMode) -> Self {
        Self::new_with(Id::new(), product, source, mode, ReleaseKind::Version, SwiftDate::now())
    }

    /// The full Swift initializer.
    pub fn new_with(
        id: Id,
        product: &Product,
        source: ReleaseSource,
        mode: ReleaseMode,
        kind: ReleaseKind,
        created_at: SwiftDate,
    ) -> Self {
        Self {
            id,
            product_id: product.id,
            product_name: product.name.clone(),
            product_slug: product.slug.clone(),
            product_type: product.product_type,
            source,
            mode,
            release_kind: Some(kind),
            state: ReleaseState::Detected,
            version: None,
            previous_version: product.current_version.clone(),
            source_sha256: None,
            svn_url: product.svn_repository_url.clone(),
            inspection: None,
            detected: None,
            validation: None,
            preview: None,
            approval: None,
            revision_before: None,
            committed_revision: None,
            verification: None,
            transitions: vec![StateTransition::new(None, ReleaseState::Detected, created_at, None)],
            errors: Vec::new(),
            manifest_path: None,
            staging_path: None,
            candidate: None,
            repository: None,
            git_commit: None,
            working_copy_modified: false,
            verified_not_committed: false,
            guidance: Vec::new(),
            release_zip_path: None,
            github_release: None,
            build: None,
            assets: None,
            code_checks: None,
            directory_status: None,
            restores_version: None,
            commit_template: None,
            created_at,
            updated_at: created_at,
        }
    }

    /// Marker embedded in the commit message so a release can be recognised on the
    /// server after an uncertain commit (timeout, crash, cancelled process).
    pub fn commit_marker(&self) -> String {
        format!("ReleaseDock-Release-ID: {}", self.id)
    }

    pub fn kind(&self) -> ReleaseKind {
        self.release_kind.unwrap_or(ReleaseKind::Version)
    }

    pub fn completed_at(&self) -> Option<SwiftDate> {
        self.transitions
            .iter()
            .rev()
            .find(|t| t.to == ReleaseState::Completed)
            .map(|t| t.at)
    }

    /// Version releases committed to WordPress.org can be followed until the directory
    /// offers them. Readme updates keep the version, so there is nothing to wait for.
    pub fn is_directory_tracked(&self) -> bool {
        if self.state != ReleaseState::Completed
            || self.mode == ReleaseMode::DryRun
            || self.kind() != ReleaseKind::Version
        {
            return false;
        }
        let Some(host) = url_host(&self.svn_url) else {
            return false;
        };
        let host = host.to_lowercase();
        host == "plugins.svn.wordpress.org" || host == "themes.svn.wordpress.org"
    }

    /// Still worth checking: tracked, not live or superseded yet, and recent.
    /// Swift's defaults are `now = Date()` and `window = 72 * 3600` ([`DIRECTORY_WAIT_WINDOW`]).
    pub fn awaits_directory(&self, now: SwiftDate, window: f64) -> bool {
        if !self.is_directory_tracked() {
            return false;
        }
        let Some(completed_at) = self.completed_at() else {
            return false;
        };
        if now.0 - completed_at.0 >= window {
            return false;
        }
        match self.directory_status.as_ref().map(|s| s.state) {
            Some(DirectoryState::Live | DirectoryState::Superseded) => false,
            Some(DirectoryState::Pending | DirectoryState::NotListed) | None => true,
        }
    }
}

/// The host of an absolute URL (`scheme://[user@]host[:port]/…`), like Foundation's
/// `URL(string:)?.host`. None when the string has no authority or an empty host.
fn url_host(url: &str) -> Option<&str> {
    let (scheme, rest) = url.split_once("://")?;
    let mut chars = scheme.chars();
    if !chars.next()?.is_ascii_alphabetic() || !chars.all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c)) {
        return None;
    }
    if url.chars().any(|c| c.is_whitespace()) {
        return None;
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host_port = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = if let Some(bracketed) = host_port.strip_prefix('[') {
        bracketed.split(']').next().unwrap_or("")
    } else {
        host_port.split(':').next().unwrap_or("")
    };
    (!host.is_empty()).then_some(host)
}

/// What can be restored: released versions older than the current one, newest first.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RestoreOptions {
    pub current_version: Option<String>,
    /// Highest version in the repository; a restore must be released above it.
    pub highest_version: Option<String>,
    pub older_versions: Vec<String>,
    pub suggested_version: Option<String>,
}

impl RestoreOptions {
    pub fn new(snapshot: &RepositorySnapshot) -> Self {
        let current_version = snapshot.current_version.clone();
        let mut candidates = snapshot.existing_versions.clone();
        candidates.extend(current_version.iter().cloned());
        let highest_version = VersionService::new().highest(&candidates);
        let mut older_versions: Vec<String> = snapshot
            .existing_versions
            .iter()
            .filter(|v| {
                current_version
                    .as_deref()
                    .is_some_and(|current| php_version::compare(v, current) < 0)
            })
            .cloned()
            .collect();
        older_versions.sort_by(|a, b| php_version::ordering(b, a));
        let suggested_version = highest_version
            .as_deref()
            .and_then(|v| VersionBumpService::suggest(Part::Patch, v));
        Self {
            current_version,
            highest_version,
            older_versions,
            suggested_version,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::url_host;

    #[test]
    fn extracts_url_hosts_like_foundation() {
        assert_eq!(
            url_host("https://plugins.svn.wordpress.org/acme/"),
            Some("plugins.svn.wordpress.org")
        );
        assert_eq!(url_host("https://user@Host.example:8443/x"), Some("Host.example"));
        assert_eq!(url_host("svn://[::1]:3690/repo"), Some("::1"));
        assert_eq!(url_host("file:///tmp/repo/acme-gallery"), None);
        assert_eq!(url_host("plugins.svn.wordpress.org/acme"), None);
        assert_eq!(url_host("https://bad host/x"), None);
    }
}
