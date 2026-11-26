//! WordPress.org directory assets. Ported from `AssetInspector.swift`.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

/// One file in a plugin's directory-assets folder (synced to SVN `/assets`).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetFile {
    pub path: String,
    pub size: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<i64>,
}

impl AssetFile {
    pub fn new(path: impl Into<String>, size: u64) -> Self {
        Self {
            path: path.into(),
            size,
            width: None,
            height: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetsReport {
    pub files: Vec<AssetFile>,
    /// Why `blueprints/blueprint.json` cannot be used; `None` when absent or valid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blueprint_problem: Option<String>,
}

impl AssetsReport {
    pub fn new(files: Vec<AssetFile>) -> Self {
        Self {
            files,
            blueprint_problem: None,
        }
    }
}

/// Reads names, sizes and pixel dimensions of the assets folder. Hidden files are
/// skipped, as the sync excludes them.
#[derive(Clone, Copy, Debug, Default)]
pub struct AssetInspector;

impl AssetInspector {
    pub fn new() -> Self {
        Self
    }

    pub fn inspect(&self, root: &Path) -> std::io::Result<AssetsReport> {
        let mut files = Vec::new();
        let mut blueprint_problem = None;
        let mut pending = vec![(root.to_path_buf(), String::new())];
        while let Some((directory, prefix)) = pending.pop() {
            // A missing or unreadable folder yields no files, like a failed enumerator.
            let Ok(read) = fs::read_dir(&directory) else { continue };
            for item in read.filter_map(Result::ok) {
                let name = item.file_name();
                let name = name.to_string_lossy();
                // `.skipsHiddenFiles`: neither hidden files nor anything inside hidden folders.
                if name.starts_with('.') {
                    continue;
                }
                let url = item.path();
                let path = format!("{prefix}{name}");
                let metadata = fs::symlink_metadata(&url)?;
                let file_type = metadata.file_type();
                if file_type.is_dir() {
                    pending.push((url, format!("{path}/")));
                    continue;
                }
                if !file_type.is_file() {
                    continue;
                }
                let mut file = AssetFile::new(path.clone(), metadata.len());
                let extension = url
                    .extension()
                    .map(|e| e.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                if ["png", "jpg", "jpeg", "gif"].contains(&extension.as_str()) {
                    if let Ok(size) = imagesize::size(&url) {
                        file.width = i64::try_from(size.width).ok();
                        file.height = i64::try_from(size.height).ok();
                    }
                }
                if path == "blueprints/blueprint.json" {
                    blueprint_problem = Self::blueprint_problem(&url);
                }
                files.push(file);
            }
        }
        files.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(AssetsReport {
            files,
            blueprint_problem,
        })
    }

    /// WordPress.org reads the blueprint with `json_decode`; it must be a JSON object.
    pub fn blueprint_problem(path: &Path) -> Option<String> {
        let Ok(data) = fs::read(path) else {
            return Some("blueprint.json cannot be read".into());
        };
        // JSONSerialization accepts a UTF-8 byte order mark.
        let data = data.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&data);
        match serde_json::from_slice::<serde_json::Value>(data) {
            Ok(serde_json::Value::Object(_)) => None,
            Ok(serde_json::Value::Array(_)) => Some("blueprint.json must contain a JSON object".into()),
            // JSONSerialization rejects top-level scalars without `.fragmentsAllowed`.
            Ok(_) | Err(_) => Some("blueprint.json is not valid JSON".into()),
        }
    }
}

/// WordPress.org plugin directory asset rules
/// (developer.wordpress.org/plugins/wordpress-org/plugin-assets/).
pub mod asset_rules {
    use super::{AssetFile, AssetsReport};
    use rd_model::{ValidationCheckId, ValidationResult};
    use regex::Regex;
    use std::collections::HashSet;
    use std::sync::LazyLock;

    pub const BANNER_LIMIT: u64 = 4 << 20;
    pub const ICON_LIMIT: u64 = 1 << 20;
    pub const SCREENSHOT_LIMIT: u64 = 10 << 20;

    const SUFFIX: &str = r"(?:-(?:rtl|[a-z]{2,3}(?:_[A-Z]{2})?))?";

    static BANNER: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(&format!(r"^banner-(772x250|1544x500)({SUFFIX})\.(?:png|jpg)$")).expect("valid regex")
    });
    static ICON: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^icon-(128x128|256x256)\.(?:png|jpg|gif)$").expect("valid regex"));
    static SCREENSHOT: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(&format!(r"^screenshot-[1-9][0-9]*{SUFFIX}\.(?:png|jpg)$")).expect("valid regex"));
    static ASSET_PREFIX: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^(banner|icon|screenshot)-").expect("valid regex"));

    const CHECK: ValidationCheckId = ValidationCheckId::DirectoryAssets;

    pub fn check(report: &AssetsReport) -> Vec<ValidationResult> {
        let mut results = Vec::new();
        let names: HashSet<&str> = report.files.iter().map(|f| f.path.as_str()).collect();
        for file in &report.files {
            let name = file.path.as_str();
            if name == "blueprints/blueprint.json" {
                continue;
            }
            if let Some((size, suffix)) = matched(&BANNER, name) {
                results.extend(check_image(file, &size, BANNER_LIMIT, "Banner"));
                let companion = format!("banner-772x250{suffix}.");
                if size == "1544x500" && !names.iter().any(|n| n.starts_with(&companion)) {
                    results.push(
                        ValidationResult::warn(
                            CHECK,
                            "The 1544×500 banner is only used together with a 772×250 banner",
                        )
                        .file(name)
                        .fix(format!("Add banner-772x250{suffix}.png or .jpg.")),
                    );
                }
            } else if let Some((size, _)) = matched(&ICON, name) {
                results.extend(check_image(file, &size, ICON_LIMIT, "Icon"));
            } else if name == "icon.svg" {
                if file.size > ICON_LIMIT {
                    results.push(ValidationResult::warn(CHECK, "Icon is over 1 MB").file(name));
                }
            } else if SCREENSHOT.is_match(name) {
                if file.size > SCREENSHOT_LIMIT {
                    results.push(
                        ValidationResult::warn(CHECK, "Screenshot is over 10 MB, the WordPress.org limit").file(name),
                    );
                }
            } else if name != name.to_lowercase() && ASSET_PREFIX.is_match(&name.to_lowercase()) {
                results.push(
                    ValidationResult::warn(
                        CHECK,
                        format!("Asset names must be lowercase; WordPress.org ignores {name}"),
                    )
                    .file(name)
                    .fix(format!("Rename it to {}.", name.to_lowercase())),
                );
            } else {
                results.push(
                    ValidationResult::warn(CHECK, format!("WordPress.org does not use {name}"))
                        .file(name)
                        .fix(
                            "Use banner-772x250, banner-1544x500, icon-128x128, icon-256x256, icon.svg or screenshot-N names (.png/.jpg).",
                        ),
                );
            }
        }
        if names.contains("icon.svg") && !names.iter().any(|n| ICON.is_match(n)) {
            results.push(
                ValidationResult::warn(CHECK, "icon.svg needs a PNG or JPG icon as a fallback")
                    .file("icon.svg")
                    .fix("Add icon-128x128.png (and icon-256x256.png)."),
            );
        }
        if let Some(problem) = &report.blueprint_problem {
            results.push(
                ValidationResult::warn(CHECK, format!("{problem}; Live Preview will not work"))
                    .file("blueprints/blueprint.json"),
            );
        }
        if results.is_empty() {
            let blueprint = if names.contains("blueprints/blueprint.json") {
                ", blueprint.json is valid"
            } else {
                ""
            };
            results.push(ValidationResult::pass(
                CHECK,
                format!(
                    "{} asset(s) follow WordPress.org names and sizes{blueprint}",
                    report.files.len()
                ),
            ));
        }
        results
    }

    /// The size (`772x250`) and, for banners, the locale/RTL suffix of a matching name.
    fn matched(regex: &Regex, name: &str) -> Option<(String, String)> {
        let captures = regex.captures(name)?;
        let size = captures.get(1)?.as_str().to_owned();
        let suffix = captures.get(2).map(|m| m.as_str().to_owned()).unwrap_or_default();
        Some((size, suffix))
    }

    fn check_image(file: &AssetFile, expected: &str, limit: u64, kind: &str) -> Vec<ValidationResult> {
        let mut results = Vec::new();
        let parts: Vec<i64> = expected.split('x').filter_map(|p| p.parse().ok()).collect();
        if let (Some(width), Some(height), &[w, h]) = (file.width, file.height, parts.as_slice()) {
            if width != w || height != h {
                results.push(
                    ValidationResult::warn(CHECK, format!("{kind} is {width}×{height} but its name says {w}×{h}"))
                        .file(&file.path)
                        .fix(format!("Export it at exactly {w}×{h} pixels.")),
                );
            }
        }
        if file.size > limit {
            results.push(
                ValidationResult::warn(
                    CHECK,
                    format!("{kind} is over {} MB, the WordPress.org limit", limit >> 20),
                )
                .file(&file.path),
            );
        }
        results
    }
}
