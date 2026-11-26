//! WordPress release branches. Ported from `WordPressBranch` in
//! `WordPressOrg/WordPressOrgService.swift`; the WordPress.org HTTP service is ported later.

use crate::text::trim_whitespace;
use rd_versioning::php_version;
use regex::Regex;
use std::sync::LazyLock;

static NUMERIC_VERSION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d+(\.\d+)*$").expect("valid regex"));

/// "Tested up to" is compared by branch (`7.1`), not by point release, the way Plugin Check
/// compares it.
pub struct WordPressBranch;

impl WordPressBranch {
    /// `7.1.5` → `7.1`, `7` → `7.0`. None for anything that is not a numeric version.
    pub fn of(version: &str) -> Option<String> {
        let trimmed = trim_whitespace(version);
        if !NUMERIC_VERSION.is_match(trimmed) {
            return None;
        }
        let parts: Vec<&str> = trimmed.split('.').collect();
        Some(format!("{}.{}", parts[0], parts.get(1).copied().unwrap_or("0")))
    }

    /// Negative when `a`'s branch is older than `b`'s, zero when equal, positive when newer.
    pub fn compare(a: &str, b: &str) -> Option<i32> {
        let a = Self::of(a)?;
        let b = Self::of(b)?;
        Some(php_version::compare(&a, &b))
    }
}
