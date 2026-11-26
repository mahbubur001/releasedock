//! Stored Git facts of a release's development folder. Ported from the data type in
//! `Git/GitService.swift`; the service itself is ported later.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitRepositoryState {
    pub is_repository: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exact_tag: Option<String>,
    /// `git status --porcelain` lines (tracked changes and untracked files).
    pub uncommitted_changes: Vec<String>,
    #[serde(rename = "remoteURL", default, skip_serializing_if = "Option::is_none")]
    pub remote_url: Option<String>,
}

impl GitRepositoryState {
    /// Swift's `GitRepositoryState.notARepository`.
    pub fn not_a_repository() -> Self {
        Self {
            is_repository: false,
            head_commit: None,
            branch: None,
            exact_tag: None,
            uncommitted_changes: Vec::new(),
            remote_url: None,
        }
    }

    pub fn is_clean(&self) -> bool {
        self.uncommitted_changes.is_empty()
    }
}
