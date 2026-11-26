//! Stored outcome of a GitHub release. Ported from the data type in
//! `GitHub/GitHubReleasePublisher.swift`; `GitHubReleaseSettings` lives in `rd-model`.

use rd_model::SwiftDate;
use serde::{Deserialize, Serialize};

pub use rd_model::GitHubReleaseSettings;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubReleaseResult {
    pub tag: String,
    /// Swift `URL`, encoded as its absolute string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asset_name: Option<String>,
    /// The release for the tag existed before (an earlier attempt or made by hand).
    pub already_existed: bool,
    pub notes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub at: SwiftDate,
}

impl GitHubReleaseResult {
    /// A result with the Swift initializer's defaults.
    pub fn new(tag: impl Into<String>, at: SwiftDate) -> Self {
        Self {
            tag: tag.into(),
            url: None,
            asset_name: None,
            already_existed: false,
            notes: Vec::new(),
            error: None,
            at,
        }
    }

    pub fn succeeded(&self) -> bool {
        self.error.is_none() && self.url.is_some()
    }
}
