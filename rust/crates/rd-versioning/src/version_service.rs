//! Version rules for WordPress releases. Ordering follows PHP `version_compare()`,
//! which is what WordPress uses to decide whether an update is available.

use crate::php_version;
use crate::text::trim_whitespace_and_newlines;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum VersionValidity {
    Valid {
        is_prerelease: bool,
    },
    /// The string cannot be trusted as a release version and requires review.
    Invalid {
        reason: String,
    },
}

impl VersionValidity {
    pub fn is_valid(&self) -> bool {
        matches!(self, Self::Valid { .. })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum VersionComparison {
    /// Candidate is higher than the current release.
    Newer,
    Same,
    Older,
    /// No current release is known (first release).
    NoCurrentRelease,
    /// One of the versions is invalid; a human must review.
    RequiresReview {
        reason: String,
    },
}

impl VersionComparison {
    pub fn allows_release(&self) -> bool {
        matches!(self, Self::Newer | Self::NoCurrentRelease)
    }
}

pub trait VersionServicing: Send + Sync {
    fn validate(&self, version: &str) -> VersionValidity;
    fn compare(&self, candidate: &str, current: Option<&str>) -> VersionComparison;
    fn highest(&self, versions: &[String]) -> Option<String>;
}

/// Words PHP's `version_compare()` orders as pre-release forms (case-sensitive,
/// exactly as php-src matches them: "Beta" or "Rc" are *unknown* and sort below "dev").
const PRERELEASE_WORDS: [&str; 7] = ["dev", "alpha", "a", "beta", "b", "RC", "rc"];
/// Patch-level forms that PHP orders *above* a plain number.
const PATCH_LEVEL_WORDS: [&str; 2] = ["pl", "p"];

#[derive(Clone, Copy, Debug, Default)]
pub struct VersionService;

impl VersionService {
    pub fn new() -> Self {
        Self
    }
}

impl VersionServicing for VersionService {
    fn validate(&self, version: &str) -> VersionValidity {
        let invalid = |reason: String| VersionValidity::Invalid { reason };
        if version.is_empty() {
            return invalid("Version is empty".into());
        }
        if version != trim_whitespace_and_newlines(version) {
            return invalid("Version has surrounding whitespace".into());
        }
        if version.chars().count() > 64 {
            return invalid("Version is longer than 64 characters".into());
        }
        // Must start with a digit: PHP orders "v1.2" *below* "1.0", which would make a
        // leading "v" silently sort as a downgrade.
        if !version.starts_with(|c: char| c.is_ascii_digit()) {
            return invalid(format!("Version must start with a digit (found \"{version}\")"));
        }
        if version
            .chars()
            .any(|c| !(c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+' | '_')))
        {
            return invalid("Version contains characters other than letters, digits, '.', '-', '+' or '_'".into());
        }
        if version.contains("..") || version.ends_with('.') {
            return invalid("Version has an empty component".into());
        }
        let canonical = php_version::canonicalize(version);
        let words: Vec<&str> = canonical
            .split('.')
            .filter(|part| !part.is_empty())
            .filter(|part| part.chars().next().is_some_and(|c| !c.is_numeric()))
            .collect();
        // php-src matches special forms by *prefix*, so "pre" sorts as "p" (a patch level,
        // newer than the plain release) and any unknown word sorts below "dev". Only exact
        // known forms are accepted; everything else needs human review.
        if let Some(word) = words
            .iter()
            .find(|word| !PRERELEASE_WORDS.contains(word) && !PATCH_LEVEL_WORDS.contains(word))
        {
            return invalid(format!(
                "Version identifier \"{word}\" has no reliable order in PHP version_compare()"
            ));
        }
        VersionValidity::Valid {
            is_prerelease: words.iter().any(|word| PRERELEASE_WORDS.contains(word)),
        }
    }

    fn compare(&self, candidate: &str, current: Option<&str>) -> VersionComparison {
        if let VersionValidity::Invalid { reason } = self.validate(candidate) {
            return VersionComparison::RequiresReview {
                reason: format!("Candidate version: {reason}"),
            };
        }
        let Some(current) = current.filter(|c| !c.is_empty()) else {
            return VersionComparison::NoCurrentRelease;
        };
        if current.to_lowercase() == "trunk" {
            return VersionComparison::NoCurrentRelease;
        }
        if let VersionValidity::Invalid { reason } = self.validate(current) {
            return VersionComparison::RequiresReview {
                reason: format!("Current version \"{current}\": {reason}"),
            };
        }
        if candidate == current {
            return VersionComparison::Same;
        }
        match php_version::compare(candidate, current) {
            1 => VersionComparison::Newer,
            -1 => VersionComparison::Older,
            // Different strings PHP considers equal (e.g. "1.0-beta" vs "1.0beta").
            _ => VersionComparison::Same,
        }
    }

    /// The highest valid version; the first one wins among versions PHP considers equal,
    /// like Swift's `max(by:)`.
    fn highest(&self, versions: &[String]) -> Option<String> {
        versions
            .iter()
            .filter(|v| self.validate(v).is_valid())
            .fold(None, |best: Option<&String>, v| match best {
                Some(b) if php_version::compare(b, v) >= 0 => Some(b),
                _ => Some(v),
            })
            .cloned()
    }
}
