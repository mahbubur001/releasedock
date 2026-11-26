//! readme.txt rules shared by plugins and themes, from the WordPress.org directory's
//! readme parser (`plugin-directory/readme/class-parser.php`) and the theme handbook.
//! All are warnings: the directory still imports the readme, but cuts what is too long.
//! Ported from `Validation/ReadmeRules.swift`.

use crate::ResultExt;
use crate::text::{capitalized, character_count, prefix, trim_whitespace};
use rd_model::{ProductType, ValidationCheckId, ValidationResult};
use rd_versioning::WordPressReadme;
use regex::Regex;
use std::sync::LazyLock;

pub const SHORT_DESCRIPTION_LIMIT: usize = 150;
/// Words per section before the parser cuts it (`maximum_field_lengths`).
pub const SECTION_WORD_LIMIT: usize = 2500;
/// Sections with a higher limit than [`SECTION_WORD_LIMIT`].
pub const LONG_SECTION_WORD_LIMIT: [(&str, usize); 3] =
    [("changelog", 5000), ("frequently asked questions", 5000), ("faq", 5000)];
/// Theme handbook limit for an upgrade notice.
pub const THEME_UPGRADE_NOTICE_LIMIT: usize = 300;
pub const SIZE_LIMIT: usize = 10 * 1024;

static MARKDOWN_LINK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[([^\]]*)\]\([^)]*\)").expect("valid regex"));
static MARKDOWN_EMPHASIS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[*_`]").expect("valid regex"));
static PLUGIN_SLUG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[a-z0-9]+(-[a-z0-9]+)*$").expect("valid regex"));

const CHECK: ValidationCheckId = ValidationCheckId::MetadataValidity;

/// The readme rules. `product_type` is `.plugin` by default in Swift.
pub fn check(
    readme: &WordPressReadme,
    file: Option<&str>,
    version: Option<&str>,
    product_type: ProductType,
) -> Vec<ValidationResult> {
    let mut results = Vec::new();
    if let Some(short) = &readme.short_description {
        let length = character_count(&plain_text(short));
        if length > SHORT_DESCRIPTION_LIMIT {
            results.push(
                ValidationResult::warn(
                    CHECK,
                    format!(
                        "The short description is {length} characters; WordPress.org cuts it at {SHORT_DESCRIPTION_LIMIT}"
                    ),
                )
                .file_opt(file)
                .fix(format!(
                    "Shorten the line under the readme headers to {SHORT_DESCRIPTION_LIMIT} characters."
                )),
            );
        }
    } else if let Some(line) = readme.unknown_header_lines.as_ref().and_then(|lines| lines.last()) {
        results.push(
            ValidationResult::warn(
                CHECK,
                format!(
                    "WordPress.org reads \"{}\" as a header because it contains \":\", so the readme has no short description",
                    prefix(line, 60)
                ),
            )
            .file_opt(file)
            .fix("Leave a blank line after the headers and avoid \":\" in the short description."),
        );
    } else {
        results.push(
            ValidationResult::warn(CHECK, "readme has no short description below the headers")
                .file_opt(file)
                .fix(format!(
                    "Add one plain sentence (up to {SHORT_DESCRIPTION_LIMIT} characters) after the header block."
                )),
        );
    }
    if !readme.sections.contains_key("description") {
        results.push(ValidationResult::warn(CHECK, "readme has no == Description == section").file_opt(file));
    }
    // `sections` is a BTreeMap, so this is Swift's `sorted(by: { $0.key < $1.key })`.
    for (title, body) in &readme.sections {
        let limit = LONG_SECTION_WORD_LIMIT
            .iter()
            .find(|(name, _)| name == title)
            .map_or(SECTION_WORD_LIMIT, |(_, limit)| *limit);
        let words = body.split_whitespace().count();
        if words > limit {
            let fix = if title == "changelog" {
                "Move older entries to changelog.txt."
            } else {
                "Shorten the section."
            };
            results.push(
                ValidationResult::warn(
                    CHECK,
                    format!(
                        "The {} section has {words} words; WordPress.org cuts it at {limit}",
                        capitalized(title)
                    ),
                )
                .file_opt(file)
                .fix(fix),
            );
        }
    }
    if product_type == ProductType::Theme {
        if let Some(version) = version {
            if let Some(notice) = readme.upgrade_notice(version) {
                let length = character_count(&plain_text(&notice));
                if length > THEME_UPGRADE_NOTICE_LIMIT {
                    results.push(
                        ValidationResult::warn(
                            CHECK,
                            format!(
                                "The upgrade notice for {version} is {length} characters; the theme handbook allows {THEME_UPGRADE_NOTICE_LIMIT}"
                            ),
                        )
                        .file_opt(file),
                    );
                }
            }
        }
    }
    if let Some(size) = readme.byte_count.filter(|size| *size > SIZE_LIMIT) {
        results.push(
            ValidationResult::warn(
                CHECK,
                format!("readme is {} KB; WordPress.org recommends under 10 KB", size / 1024),
            )
            .file_opt(file)
            .fix("Keep the latest changelog entries in the readme and move older ones to changelog.txt."),
        );
    }
    results
}

/// Strips Markdown links/emphasis so lengths count what readers see.
pub fn plain_text(s: &str) -> String {
    let text = MARKDOWN_LINK.replace_all(s, "${1}");
    let text = MARKDOWN_EMPHASIS.replace_all(&text, "");
    text.trim().to_owned()
}

/// `Requires Plugins`: comma-separated WordPress.org slugs (lowercase letters, digits
/// and hyphens). Paths like `dir/file.php` are not supported by core.
pub fn check_requires_plugins(value: Option<&str>, slug: &str, file: Option<&str>) -> Vec<ValidationResult> {
    let Some(value) = value else {
        return Vec::new();
    };
    let mut results = Vec::new();
    let slugs = value.split(',').map(trim_whitespace).filter(|s| !s.is_empty());
    for dependency in slugs {
        if !PLUGIN_SLUG.is_match(dependency) {
            results.push(
                ValidationResult::warn(
                    CHECK,
                    format!(
                        "Requires Plugins entry \"{dependency}\" is not a WordPress.org slug; WordPress ignores it"
                    ),
                )
                .file_opt(file)
                .fix("Use the plugin's directory slug, e.g. woocommerce."),
            );
        } else if dependency == slug {
            results.push(ValidationResult::warn(CHECK, "Requires Plugins lists the plugin itself").file_opt(file));
        }
    }
    results
}
