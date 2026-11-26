//! What the WordPress.org directory offers, as used by `DirectoryStatus`. Ported from the
//! data type in `WordPressOrg/WordPressOrgService.swift`; the service is ported later.

use serde::{Deserialize, Serialize};

/// What the plugin or theme directory currently offers to sites.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryListing {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tested: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_updated: Option<String>,
}

impl DirectoryListing {
    /// Swift's `DirectoryListing(version:)`.
    pub fn new(version: Option<String>) -> Self {
        Self {
            version,
            tested: None,
            last_updated: None,
        }
    }
}
