//! WordPress.org theme release rules. Ported from the validation part of
//! `Release/ThemeReleaseStrategy.swift`.
//!
//! Themes have no trunk/tags: every version is its own top-level directory
//! (`<slug>/<version>/`). A new version is created by copying the previous version directory
//! (`svn cp 1.0.1 1.0.2`), replacing its contents, and committing exactly one new version
//! directory.
//!
//! WordPress.org imports the commit *afterwards* and runs Theme Check; a failure is
//! emailed to the author and that version never goes live, while the directory stays
//! in SVN and cannot be overwritten. Validation is therefore deliberately strict.

use crate::strategy::ReleaseStrategy;
use crate::text::{capitalized, lowercased, path_extension};
use crate::{ResultExt, readme_rules};
use rd_model::{
    EntryKind, PackageInspection, Product, ProductType, ValidationCheckId, ValidationResult, ValidationSeverity,
    ValidationStatus,
};
use rd_versioning::DetectedProduct;
use regex::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;

pub const REQUIRED_HEADERS: [&str; 4] = ["Theme Name", "Description", "Version", "Tags"];
pub const REVIEW_HEADERS: [&str; 7] = [
    "Author",
    "Requires at least",
    "Tested up to",
    "Requires PHP",
    "License",
    "License URI",
    "Text Domain",
];
pub const DISALLOWED_EXTENSIONS: [&str; 4] = ["sql", "sh", "dat", "wie"];
pub const RESOURCE_EXTENSIONS: [&str; 10] = [
    "png", "jpg", "jpeg", "gif", "svg", "webp", "woff", "woff2", "ttf", "otf",
];
pub const ALLOWED_XML: [&str; 3] = ["wpml-config.xml", "loco.xml", "phpcs.xml"];

static NUMERIC_VERSION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d+(\.\d+)*$").expect("valid regex"));
static TESTED_UP_TO: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d+\.\d(\.\d+)?$").expect("valid regex"));
static MISSPELLED_WORDPRESS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bWordpress\b").expect("valid regex"));
static LOWERCASE_WORDPRESS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bwordpress\b").expect("valid regex"));

/// Swift: `#"\bWordpress\b|\bwordpress\b(?!\.)"#`. `regex` has no look-ahead, so a
/// lowercase match counts only when the next character is not `.` (`wordpress.org`).
fn misspells_wordpress(text: &str) -> bool {
    MISSPELLED_WORDPRESS.is_match(text)
        || LOWERCASE_WORDPRESS
            .find_iter(text)
            .any(|m| !text[m.end()..].starts_with('.'))
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ThemeReleaseStrategy;

impl ThemeReleaseStrategy {
    pub fn new() -> Self {
        Self
    }

    /// Top-level SVN directories that are theme versions.
    pub fn is_version_directory(name: &str) -> bool {
        name.chars().next().is_some_and(char::is_numeric)
    }
}

impl ReleaseStrategy for ThemeReleaseStrategy {
    fn product_type(&self) -> ProductType {
        ProductType::Theme
    }

    fn release_path(&self, version: &str) -> String {
        version.to_owned()
    }

    fn allowed_change_prefixes(&self, version: &str) -> Vec<String> {
        vec![version.to_owned()]
    }

    fn validate_version_format(&self, version: &str) -> Vec<ValidationResult> {
        // Importer rule: ^\d+(\.\d+)*$, not all zeros, equal to the directory name.
        if !NUMERIC_VERSION.is_match(version) {
            return vec![
                ValidationResult::fail(
                    ValidationCheckId::VersionValidity,
                    format!("Theme versions must contain only numbers and dots (found \"{version}\")"),
                )
                .fix("Use a version like 1.2.3."),
            ];
        }
        // Swift `Int($0) == 0`: a component that does not fit an Int is not zero.
        if version.split('.').all(|part| part.parse::<i64>() == Ok(0)) {
            return vec![ValidationResult::fail(
                ValidationCheckId::VersionValidity,
                "Theme version cannot be all zeros",
            )];
        }
        Vec::new()
    }

    fn validate_metadata(&self, detected: &DetectedProduct, product: &Product) -> Vec<ValidationResult> {
        use ValidationCheckId::{ChangelogEntry, MetadataValidity, TestedUpTo};
        let mut results = Vec::new();
        for header in REQUIRED_HEADERS {
            if !detected.headers.contains_key(header) {
                results.push(
                    ValidationResult::fail(
                        MetadataValidity,
                        format!(
                            "style.css is missing the \"{header}\" header, which the WordPress.org importer requires"
                        ),
                    )
                    .file("style.css"),
                );
            }
        }
        for header in REVIEW_HEADERS {
            if !detected.headers.contains_key(header) {
                results.push(
                    ValidationResult::warn(
                        MetadataValidity,
                        format!("style.css is missing the \"{header}\" header required by theme review"),
                    )
                    .file("style.css"),
                );
            }
        }
        // Theme review: names must not use WordPress, Theme or Twenty*; spell WordPress correctly.
        if let Some(name) = &detected.name {
            let lower = lowercased(name);
            let words: Vec<&str> = lower
                .split(|c: char| !c.is_alphabetic() && !c.is_numeric())
                .filter(|word| !word.is_empty())
                .collect();
            if words.contains(&"wordpress") || words.contains(&"theme") || words.iter().any(|w| w.starts_with("twenty"))
            {
                results.push(
                    ValidationResult::warn(
                        MetadataValidity,
                        "Theme names must not use \"WordPress\", \"Theme\" or \"Twenty\" (theme review requirement)",
                    )
                    .file("style.css"),
                );
            }
        }
        let public_text = [
            detected.name.as_deref(),
            detected.headers.get("Description").map(String::as_str),
            detected.readme.as_ref().and_then(|r| r.short_description.as_deref()),
        ];
        if public_text.into_iter().flatten().any(misspells_wordpress) {
            results.push(
                ValidationResult::warn(
                    MetadataValidity,
                    "Spell \"WordPress\" with a capital W and P in the name, description and readme",
                )
                .file("style.css"),
            );
        }
        if let (Some(size), Some(file)) = (detected.screenshot_size, detected.screenshot_file.as_deref()) {
            if size.width > 1200 || size.height > 900 {
                results.push(
                    ValidationResult::warn(
                        MetadataValidity,
                        format!(
                            "{file} is {}×{}; theme review allows at most 1200×900",
                            size.width, size.height
                        ),
                    )
                    .file(file)
                    .fix("Export it at 1200×900."),
                );
            }
            if size.width * 3 != size.height * 4 {
                results.push(
                    ValidationResult::warn(
                        MetadataValidity,
                        format!(
                            "{file} is {}×{}, not 4:3 as theme review requires",
                            size.width, size.height
                        ),
                    )
                    .file(file)
                    .fix("Export it at 1200×900."),
                );
            }
        }
        if let Some(parent) = detected.parent_theme() {
            results.push(ValidationResult::new(
                MetadataValidity,
                ValidationStatus::Passed,
                ValidationSeverity::Info,
                format!("Child theme of \"{parent}\""),
            ));
        }
        if let Some(tested) = detected
            .headers
            .get("Tested up to")
            .filter(|t| !TESTED_UP_TO.is_match(t))
        {
            results.push(
                ValidationResult::warn(
                    TestedUpTo,
                    format!("Tested up to \"{tested}\" is not a WordPress version number"),
                )
                .file("style.css"),
            );
        }
        if let Some(readme) = &detected.readme {
            for field in [
                "contributors",
                "requires at least",
                "tested up to",
                "requires php",
                "license",
                "license uri",
            ] {
                if !readme.fields.contains_key(field) {
                    results.push(
                        ValidationResult::warn(
                            MetadataValidity,
                            format!("readme.txt is missing \"{}\"", capitalized(field)),
                        )
                        .file_opt(detected.readme_file.as_deref()),
                    );
                }
            }
            results.extend(readme_rules::check(
                readme,
                detected.readme_file.as_deref(),
                detected.version.as_deref(),
                ProductType::Theme,
            ));
            if product.validation.require_changelog_entry {
                if let Some(version) = detected.version.as_deref().filter(|v| !readme.changelog_mentions(v)) {
                    results.push(
                        ValidationResult::warn(
                            ChangelogEntry,
                            format!("readme.txt changelog has no entry for {version}"),
                        )
                        .file_opt(detected.readme_file.as_deref()),
                    );
                }
            }
        }
        if results.iter().all(|r| r.status == ValidationStatus::Passed) {
            results.push(ValidationResult::pass(
                MetadataValidity,
                "style.css headers are complete",
            ));
        }
        results
    }

    fn validate_required_files(
        &self,
        inspection: &PackageInspection,
        detected: &DetectedProduct,
    ) -> Vec<ValidationResult> {
        use ValidationCheckId::RequiredFiles;
        let files: HashSet<&str> = inspection
            .included_files()
            .into_iter()
            .map(|entry| entry.path.as_str())
            .collect();
        let mut results = Vec::new();
        if files.contains("style.css") {
            results.push(ValidationResult::pass(RequiredFiles, "style.css present"));
        } else {
            results.push(ValidationResult::fail(
                RequiredFiles,
                "style.css must be at the theme root",
            ));
        }
        let is_block_theme = files.contains("templates/index.html") || files.contains("block-templates/index.html");
        if files.contains("index.php") || is_block_theme {
            results.push(ValidationResult::pass(
                RequiredFiles,
                if is_block_theme {
                    "Block theme templates/index.html present"
                } else {
                    "index.php present"
                },
            ));
        } else {
            results.push(ValidationResult::fail(
                RequiredFiles,
                "A theme needs index.php (classic) or templates/index.html (block theme)",
            ));
        }
        if is_block_theme && !files.contains("theme.json") {
            results.push(ValidationResult::warn(
                RequiredFiles,
                "Block themes are required to include theme.json",
            ));
        }
        if ["screenshot.png", "screenshot.jpg", "screenshot.jpeg", "screenshot.gif"]
            .iter()
            .any(|name| files.contains(name))
        {
            results.push(ValidationResult::pass(RequiredFiles, "Screenshot present"));
        } else {
            results.push(ValidationResult::fail(
                RequiredFiles,
                "screenshot.png (or .jpg/.jpeg/.gif) is required at the theme root",
            ));
        }
        let bundled = files
            .iter()
            .filter(|path| {
                let ext = lowercased(path_extension(path));
                RESOURCE_EXTENSIONS.contains(&ext.as_str()) && !lowercased(path).starts_with("screenshot.")
            })
            .count();
        let has_resources = detected
            .readme
            .as_ref()
            .is_some_and(|readme| readme.sections.contains_key("resources"));
        if bundled > 0 && !has_resources {
            results.push(
                ValidationResult::warn(
                    RequiredFiles,
                    format!(
                        "The theme bundles {bundled} image or font file(s), but readme.txt has no == Resources == section listing their licenses"
                    ),
                )
                .file_opt(detected.readme_file.as_deref())
                .fix("Add == Resources == with each file, its author, license and source."),
            );
        }
        if files.iter().any(|path| lowercased(path) == "readme.txt") {
            results.push(ValidationResult::pass(RequiredFiles, "readme.txt present"));
        } else {
            results.push(ValidationResult::fail(
                RequiredFiles,
                "readme.txt is required for WordPress.org themes",
            ));
        }
        results
    }

    fn validate_forbidden_files(&self, inspection: &PackageInspection) -> Vec<ValidationResult> {
        let mut results = Vec::new();
        for entry in &inspection.included_entries {
            let name = entry.path.rsplit('/').next().unwrap_or(&entry.path);
            let lower = lowercased(name);
            let ext = path_extension(&lower);
            let problem = if name.starts_with('.') {
                Some("Hidden files are not allowed in WordPress.org themes".to_owned())
            } else if lower == "favicon.ico" {
                Some("favicon.ico is not allowed in WordPress.org themes".to_owned())
            } else if entry.kind == EntryKind::File && DISALLOWED_EXTENSIONS.contains(&ext) {
                Some(format!(".{ext} files are not allowed in WordPress.org themes"))
            } else if entry.kind == EntryKind::File && ext == "xml" && !ALLOWED_XML.contains(&lower.as_str()) {
                Some(
                    "XML files other than wpml-config.xml, loco.xml and phpcs.xml are not allowed in WordPress.org themes"
                        .to_owned(),
                )
            } else {
                None
            };
            if let Some(problem) = problem {
                results.push(
                    ValidationResult::fail(ValidationCheckId::ForbiddenFiles, problem)
                        .file(&entry.path)
                        .fix("Remove it or add an exclusion pattern. A rejected theme version cannot be re-committed."),
                );
            }
        }
        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spelling_check_skips_wordpress_org() {
        assert!(misspells_wordpress("A clean Wordpress blog theme."));
        assert!(misspells_wordpress("for wordpress sites"));
        assert!(misspells_wordpress("wordpress.org and wordpress"));
        assert!(!misspells_wordpress("Works on wordpress.org and WordPress."));
        assert!(!misspells_wordpress("wordpressy"));
    }

    #[test]
    fn version_directories_start_with_a_number() {
        assert!(ThemeReleaseStrategy::is_version_directory("1.0.2"));
        assert!(!ThemeReleaseStrategy::is_version_directory("trunk"));
        assert!(!ThemeReleaseStrategy::is_version_directory(""));
    }
}
