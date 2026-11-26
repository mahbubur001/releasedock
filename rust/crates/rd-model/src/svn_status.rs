//! Status of one item in `svn status`, stored in working-copy changes.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SvnItemStatus {
    Added,
    Conflicted,
    Deleted,
    External,
    Ignored,
    Incomplete,
    Merged,
    Missing,
    Modified,
    None,
    Normal,
    Obstructed,
    Replaced,
    Unversioned,
}

impl SvnItemStatus {
    pub fn from_raw(raw: &str) -> Option<Self> {
        Some(match raw {
            "added" => Self::Added,
            "conflicted" => Self::Conflicted,
            "deleted" => Self::Deleted,
            "external" => Self::External,
            "ignored" => Self::Ignored,
            "incomplete" => Self::Incomplete,
            "merged" => Self::Merged,
            "missing" => Self::Missing,
            "modified" => Self::Modified,
            "none" => Self::None,
            "normal" => Self::Normal,
            "obstructed" => Self::Obstructed,
            "replaced" => Self::Replaced,
            "unversioned" => Self::Unversioned,
            _ => return None,
        })
    }
}
