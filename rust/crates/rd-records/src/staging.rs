//! The staged release candidate. Ported from the data type in
//! `Release/ReleaseStagingService.swift`; the service itself is ported later.

use rd_model::{EntryKind, PackageEntry};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A clean release candidate: exactly the files that will be published.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StagedCandidate {
    pub root_path: String,
    pub entries: Vec<PackageEntry>,
    pub tree_digest: String,
}

impl StagedCandidate {
    pub fn new(root_path: impl Into<String>, entries: Vec<PackageEntry>, tree_digest: impl Into<String>) -> Self {
        Self {
            root_path: root_path.into(),
            entries,
            tree_digest: tree_digest.into(),
        }
    }

    /// Swift's `rootURL`.
    pub fn root_dir(&self) -> PathBuf {
        PathBuf::from(&self.root_path)
    }

    pub fn files(&self) -> Vec<&PackageEntry> {
        self.entries.iter().filter(|e| e.kind == EntryKind::File).collect()
    }
}
