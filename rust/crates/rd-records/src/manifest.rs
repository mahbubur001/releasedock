//! Ported from `Release/ReleaseManifest.swift`.

use crate::release::{ReleaseKind, ReleaseRecord, ReleaseState};
use crate::staging::StagedCandidate;
use crate::swift_json;
use rd_model::iso8601;
use rd_model::{Id, ProductType, ReleaseMode, SourceKind, SwiftDate, ValidationResult};
use serde::{Deserialize, Serialize};

/// `ReleaseManifest.File` in Swift.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestFile {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

/// Release manifest written next to each release's report. It deliberately has no
/// field that could hold a credential.
///
/// Unlike the release record, the manifest encodes dates as ISO 8601 strings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseManifest {
    pub schema_version: i64,
    #[serde(rename = "releaseID")]
    pub release_id: Id,
    pub product: String,
    pub slug: String,
    #[serde(rename = "type")]
    pub product_type: ProductType,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_version: Option<String>,
    pub mode: ReleaseMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<ReleaseKind>,
    /// The older version whose code this release restores.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restores_version: Option<String>,
    pub source_kind: SourceKind,
    pub source_path: String,
    #[serde(rename = "sourceSHA256")]
    pub source_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_commit: Option<String>,
    #[serde(rename = "svnURL")]
    pub svn_url: String,
    pub release_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub svn_revision_before: Option<i64>,
    pub validation: Vec<ValidationResult>,
    pub state: ReleaseState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_revision: Option<i64>,
    pub files: Vec<ManifestFile>,
    #[serde(with = "iso8601")]
    pub generated_at: SwiftDate,
    pub generator: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("The release manifest could not be encoded: {0}")]
    Encoding(#[from] serde_json::Error),
}

impl ReleaseManifest {
    /// Builds the manifest of `record`. Swift's `now` defaults to `Date()`.
    pub fn make(
        record: &ReleaseRecord,
        candidate: Option<&StagedCandidate>,
        release_path: &str,
        git_commit: Option<&str>,
        generator: &str,
        now: SwiftDate,
    ) -> Self {
        Self {
            schema_version: 1,
            release_id: record.id,
            product: record.product_name.clone(),
            slug: record.product_slug.clone(),
            product_type: record.product_type,
            version: record.version.clone().unwrap_or_default(),
            previous_version: record.previous_version.clone(),
            mode: record.mode,
            kind: Some(record.kind()),
            restores_version: record.restores_version.clone(),
            source_kind: record.source.kind(),
            source_path: record.source.path().to_owned(),
            source_sha256: record.source_sha256.clone().unwrap_or_default(),
            candidate_digest: candidate.map(|c| c.tree_digest.clone()),
            git_commit: git_commit.map(str::to_owned),
            svn_url: record.svn_url.clone(),
            release_path: release_path.to_owned(),
            svn_revision_before: record.revision_before,
            validation: record
                .validation
                .as_ref()
                .map(|v| v.results.clone())
                .unwrap_or_default(),
            state: record.state,
            final_revision: record.committed_revision,
            files: candidate
                .map(|c| {
                    c.files()
                        .into_iter()
                        .map(|f| ManifestFile {
                            path: f.path.clone(),
                            size: f.size,
                            sha256: f.sha256.clone().unwrap_or_default(),
                        })
                        .collect()
                })
                .unwrap_or_default(),
            generated_at: now,
            generator: generator.to_owned(),
        }
    }

    /// Pretty-printed JSON with sorted keys and unescaped slashes, laid out like Swift's
    /// `JSONEncoder` output; dates are ISO 8601.
    pub fn encoded(&self) -> Result<Vec<u8>, ManifestError> {
        let value = serde_json::to_value(self)?;
        Ok(swift_json::to_pretty_sorted(&value).into_bytes())
    }
}
