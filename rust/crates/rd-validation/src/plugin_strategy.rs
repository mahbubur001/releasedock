//! WordPress.org plugin release rules. Ported from the validation part of
//! `Release/PluginReleaseStrategy.swift`.
//!
//! ```text
//! <slug>/
//! ├── assets/   banners, icons, screenshots (optional sync)
//! ├── tags/     one directory per released version
//! └── trunk/    latest code; trunk/readme.txt "Stable tag" selects the served tag
//! ```
//!
//! The release is one atomic commit containing the trunk update and `svn cp trunk
//! tags/<version>` (the approach used by 10up/action-wordpress-plugin-deploy).

use crate::readme_rules;
use crate::strategy::ReleaseStrategy;
use crate::text::lowercased;
use rd_model::{PackageInspection, Product, ProductType, ValidationCheckId, ValidationResult};
use rd_versioning::DetectedProduct;
use regex::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;

/// The pattern the WordPress.org importer expects; anything else only warns there.
pub const IMPORTER_VERSION_PATTERN: &str = r"^\d+(?:\.\d+)*(?:-(?:rc|beta|alpha)(?:\.?\d+)?)?$";

static IMPORTER_VERSION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!("(?i){IMPORTER_VERSION_PATTERN}")).expect("valid regex"));
static TESTED_UP_TO: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d+\.\d(\.\d+)?$").expect("valid regex"));
static REQUIRES_PHP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d+(\.\d+){1,2}$").expect("valid regex"));

#[derive(Clone, Copy, Debug, Default)]
pub struct PluginReleaseStrategy;

impl PluginReleaseStrategy {
    pub fn new() -> Self {
        Self
    }

    /// `readme.txt` (preferred) or `readme.md`, matched case-insensitively like WordPress.org.
    pub fn readme_name(files: &[String]) -> Option<&str> {
        ["readme.txt", "readme.md"]
            .iter()
            .find_map(|wanted| files.iter().find(|name| lowercased(name) == *wanted))
            .map(String::as_str)
    }
}

impl ReleaseStrategy for PluginReleaseStrategy {
    fn product_type(&self) -> ProductType {
        ProductType::Plugin
    }

    fn release_path(&self, version: &str) -> String {
        format!("tags/{version}")
    }

    fn allowed_change_prefixes(&self, version: &str) -> Vec<String> {
        vec!["trunk".into(), format!("tags/{version}"), "assets".into()]
    }

    fn validate_version_format(&self, version: &str) -> Vec<ValidationResult> {
        if !IMPORTER_VERSION.is_match(version) {
            return vec![ValidationResult::warn(
                ValidationCheckId::VersionValidity,
                format!(
                    "WordPress.org expects plugin versions like 1.2.3 or 1.2.3-beta1; \"{version}\" will raise an importer warning"
                ),
            )];
        }
        Vec::new()
    }

    fn validate_metadata(&self, detected: &DetectedProduct, product: &Product) -> Vec<ValidationResult> {
        use ValidationCheckId::{ChangelogEntry, MetadataValidity, TestedUpTo};
        let mut results = Vec::new();
        for problem in &detected.problems {
            results.push(
                ValidationResult::fail(MetadataValidity, problem)
                    .fix("Set the main plugin file in the product settings."),
            );
        }
        let Some(main_file) = detected.main_file.as_deref() else {
            return results;
        };
        if detected.name.is_none() {
            results.push(ValidationResult::fail(MetadataValidity, "Missing \"Plugin Name:\" header").file(main_file));
        }
        if !detected.headers.contains_key("License") {
            results.push(
                ValidationResult::warn(MetadataValidity, "Main plugin file has no License header").file(main_file),
            );
        }

        let (Some(readme), Some(readme_file)) = (detected.readme.as_ref(), detected.readme_file.as_deref()) else {
            results.push(
                ValidationResult::fail(MetadataValidity, "readme.txt is missing")
                    .fix("Add a readme.txt with a Stable tag header."),
            );
            return results;
        };
        if lowercased(readme_file) == "readme.md" {
            results.push(
                ValidationResult::warn(MetadataValidity, "Using readme.md; WordPress.org prefers readme.txt")
                    .file(readme_file),
            );
        }
        if readme.name.is_none() {
            results.push(
                ValidationResult::fail(MetadataValidity, "readme has no \"=== Plugin Name ===\" title line")
                    .file(readme_file),
            );
        }

        // Stable tag: WordPress.org reads it from trunk/readme.txt; a missing or wrong value
        // silently serves trunk (or blocks with Release Confirmation).
        let version = detected.version.as_deref().unwrap_or("");
        match readme.stable_tag().as_deref() {
            None | Some("") => results.push(
                ValidationResult::fail(MetadataValidity, "readme Stable tag is missing")
                    .file(readme_file)
                    .fix(format!("Add \"Stable tag: {version}\".")),
            ),
            Some("trunk") => results.push(
                ValidationResult::fail(
                    MetadataValidity,
                    "Stable tag is \"trunk\", which WordPress.org discourages and prohibits for new plugins",
                )
                .file(readme_file)
                .fix(format!("Set \"Stable tag: {version}\".")),
            ),
            Some(tag) => {
                if tag == version {
                    results.push(ValidationResult::pass(
                        MetadataValidity,
                        format!("Stable tag {tag} matches the plugin version"),
                    ));
                } else {
                    results.push(
                        ValidationResult::fail(
                            MetadataValidity,
                            format!("Stable tag {tag} does not match the plugin version {version}"),
                        )
                        .file(readme_file)
                        .fix(format!("Set \"Stable tag: {version}\".")),
                    );
                }
                if let Some(raw) = readme.raw_stable_tag().filter(|raw| *raw != tag) {
                    results.push(
                        ValidationResult::warn(
                            MetadataValidity,
                            format!("Stable tag \"{raw}\" is read by WordPress.org as \"{tag}\""),
                        )
                        .file(readme_file),
                    );
                }
            }
        }

        if let Some(tested) = readme.tested_up_to() {
            if !TESTED_UP_TO.is_match(tested) {
                results.push(
                    ValidationResult::warn(
                        TestedUpTo,
                        format!("Tested up to \"{tested}\" is not a WordPress version number like 6.9"),
                    )
                    .file(readme_file),
                );
            } else {
                results.push(ValidationResult::pass(TestedUpTo, format!("Tested up to {tested}")));
            }
        } else {
            results.push(ValidationResult::warn(TestedUpTo, "readme has no \"Tested up to\" header").file(readme_file));
        }
        if let Some(requires_php) = readme.requires_php().filter(|value| !REQUIRES_PHP.is_match(value)) {
            results.push(
                ValidationResult::warn(
                    MetadataValidity,
                    format!("Requires PHP \"{requires_php}\" is not a version number and will be ignored"),
                )
                .file(readme_file),
            );
        }
        // The main plugin file overrides these readme headers on WordPress.org.
        for (header, readme_value) in [
            ("Requires at least", readme.requires_at_least()),
            ("Requires PHP", readme.requires_php()),
        ] {
            if let (Some(plugin_value), Some(readme_value)) = (detected.headers.get(header), readme_value) {
                if plugin_value != readme_value {
                    results.push(
                        ValidationResult::warn(
                            MetadataValidity,
                            format!(
                                "{header} is {plugin_value} in {main_file} but {readme_value} in the readme; the plugin file wins"
                            ),
                        )
                        .file(main_file),
                    );
                }
            }
        }
        if readme.license().is_none() {
            results.push(ValidationResult::warn(MetadataValidity, "readme has no License header").file(readme_file));
        }
        if readme.contributors().is_empty() {
            results.push(ValidationResult::warn(MetadataValidity, "readme has no Contributors").file(readme_file));
        }
        results.extend(readme_rules::check(
            readme,
            Some(readme_file),
            detected.version.as_deref(),
            ProductType::Plugin,
        ));
        results.extend(readme_rules::check_requires_plugins(
            detected.headers.get("Requires Plugins").map(String::as_str),
            &product.slug,
            Some(main_file),
        ));
        let tags = readme
            .tags()
            .into_iter()
            .filter(|tag| !["plugin", "wordpress"].contains(&lowercased(tag).as_str()))
            .count();
        if tags > 5 {
            results.push(
                ValidationResult::warn(
                    MetadataValidity,
                    format!("readme has {tags} tags; WordPress.org keeps only the first 5"),
                )
                .file(readme_file),
            );
        }
        if product.validation.require_changelog_entry {
            if readme.changelog_mentions(version) {
                results.push(ValidationResult::pass(
                    ChangelogEntry,
                    format!("Changelog has an entry for {version}"),
                ));
            } else {
                results.push(
                    ValidationResult::warn(ChangelogEntry, format!("Changelog has no entry for {version}"))
                        .file(readme_file)
                        .fix(format!("Add a \"= {version} =\" entry under == Changelog ==.")),
                );
            }
        }
        results
    }

    fn validate_required_files(
        &self,
        inspection: &PackageInspection,
        detected: &DetectedProduct,
    ) -> Vec<ValidationResult> {
        use ValidationCheckId::RequiredFiles;
        let root_files: HashSet<&str> = inspection
            .included_files()
            .into_iter()
            .map(|entry| entry.path.as_str())
            .filter(|path| !path.contains('/'))
            .collect();
        let mut results = Vec::new();
        match detected.main_file.as_deref() {
            Some(main) if root_files.contains(main) => results.push(ValidationResult::pass(
                RequiredFiles,
                format!("Main plugin file {main} is at the package root"),
            )),
            _ => results.push(ValidationResult::fail(
                RequiredFiles,
                "Main plugin file must be at the package root",
            )),
        }
        match detected.readme_file.as_deref() {
            Some(readme) if root_files.contains(readme) => {
                results.push(ValidationResult::pass(RequiredFiles, format!("{readme} present")));
            }
            _ => results.push(ValidationResult::fail(
                RequiredFiles,
                "readme.txt must be at the package root",
            )),
        }
        results
    }

    fn validate_forbidden_files(&self, inspection: &PackageInspection) -> Vec<ValidationResult> {
        // Screenshots, banners and icons belong in /assets, not in trunk.
        inspection
            .included_files()
            .into_iter()
            .filter(|entry| {
                let name = lowercased(&entry.path);
                !name.contains('/')
                    && (name.starts_with("screenshot-") || name.starts_with("banner-") || name.starts_with("icon-"))
                    && [".png", ".jpg", ".jpeg", ".gif", ".svg"]
                        .iter()
                        .any(|suffix| name.ends_with(suffix))
            })
            .map(|entry| {
                ValidationResult::warn(
                    ValidationCheckId::ForbiddenFiles,
                    "Directory assets belong in SVN /assets, not in the plugin",
                )
                .file(&entry.path)
                .fix("Move it to the product's assets folder.")
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readme_name_prefers_readme_txt() {
        let files = ["README.md".to_owned(), "ReadMe.TXT".to_owned(), "x.php".to_owned()];
        assert_eq!(PluginReleaseStrategy::readme_name(&files), Some("ReadMe.TXT"));
        assert_eq!(PluginReleaseStrategy::readme_name(&files[..1]), Some("README.md"));
        assert_eq!(PluginReleaseStrategy::readme_name(&files[2..]), None);
    }
}
