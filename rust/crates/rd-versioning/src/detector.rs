//! Detects the plugin or theme at a package root from its file headers.

use crate::headers;
use crate::readme::{self, WordPressReadme};
use rd_model::ProductType;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PixelSize {
    pub width: i64,
    pub height: i64,
}

impl PixelSize {
    pub fn new(width: i64, height: i64) -> Self {
        Self { width, height }
    }

    /// Pixel size of a PNG, JPEG or GIF, or None when it cannot be read.
    pub fn of_image_at(path: &Path) -> Option<Self> {
        let size = imagesize::size(path).ok()?;
        Some(Self {
            width: i64::try_from(size.width).ok()?,
            height: i64::try_from(size.height).ok()?,
        })
    }
}

/// What a package root says about itself. The version always comes from file
/// headers inside the package, never from a ZIP or folder name.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedProduct {
    #[serde(rename = "type")]
    pub product_type: ProductType,
    /// `Plugin Name:` or `Theme Name:`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_domain: Option<String>,
    /// Main plugin file or `style.css`, relative to the package root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub main_file: Option<String>,
    pub headers: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub readme: Option<WordPressReadme>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub readme_file: Option<String>,
    /// Problems encountered while detecting (ambiguous main file, unreadable headers…).
    pub problems: Vec<String>,
    /// Themes: the root screenshot and its pixel size, when readable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screenshot_file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screenshot_size: Option<PixelSize>,
}

impl DetectedProduct {
    /// A detection result with only the type set, like the Swift initializer's defaults.
    pub fn new(product_type: ProductType) -> Self {
        Self {
            product_type,
            name: None,
            version: None,
            text_domain: None,
            main_file: None,
            headers: BTreeMap::new(),
            readme: None,
            readme_file: None,
            problems: Vec::new(),
            screenshot_file: None,
            screenshot_size: None,
        }
    }

    /// `Template:` header present → child theme.
    pub fn parent_theme(&self) -> Option<&str> {
        match self.product_type {
            ProductType::Theme => self.headers.get("Template").map(String::as_str),
            ProductType::Plugin => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProductDetectionError {
    #[error("Not a directory: {0}")]
    NotADirectory(String),
    #[error(
        "No WordPress plugin (PHP file with a \"Plugin Name:\" header) or theme (style.css with a \"Theme Name:\" header) was found at the package root."
    )]
    NoProductFound,
    #[error("The package contains both a plugin header and a theme style.css header.")]
    AmbiguousType,
    /// A file or directory could not be read (Swift rethrows the Foundation error).
    #[error("{0}")]
    Io(String),
}

fn io_error(path: &Path) -> impl FnOnce(io::Error) -> ProductDetectionError + '_ {
    move |error| ProductDetectionError::Io(format!("{}: {error}", path.display()))
}

/// A regular file that is not a symbolic link (`isRegularFile && !isSymbolicLink`).
fn is_plain_file(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_file())
}

fn has_php_extension(name: &str) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|ext| ext.to_string_lossy().to_lowercase() == "php")
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ProductDetector;

impl ProductDetector {
    pub fn new() -> Self {
        Self
    }

    /// Detects a plugin or theme at `root`. Pass `expected_type` to resolve packages that
    /// look like both, and `main_plugin_file` to pin the plugin main file.
    pub fn detect(
        &self,
        root: &Path,
        expected_type: Option<ProductType>,
        main_plugin_file: Option<&str>,
        slug: Option<&str>,
    ) -> Result<DetectedProduct, ProductDetectionError> {
        if !fs::metadata(root).is_ok_and(|m| m.is_dir()) {
            return Err(ProductDetectionError::NotADirectory(
                root.to_string_lossy().into_owned(),
            ));
        }
        let plugin = self.detect_plugin(root, main_plugin_file, slug)?;
        let theme = self.detect_theme(root)?;

        match (plugin, theme, expected_type) {
            (Some(p), None, _) => Ok(p),
            (None, Some(t), _) => Ok(t),
            (Some(p), Some(_), Some(ProductType::Plugin)) => Ok(p),
            (Some(_), Some(t), Some(ProductType::Theme)) => Ok(t),
            (Some(_), Some(_), None) => Err(ProductDetectionError::AmbiguousType),
            (None, None, _) => Err(ProductDetectionError::NoProductFound),
        }
    }

    // MARK: Plugin

    pub(crate) fn detect_plugin(
        &self,
        root: &Path,
        main_plugin_file: Option<&str>,
        slug: Option<&str>,
    ) -> Result<Option<DetectedProduct>, ProductDetectionError> {
        // WordPress scans PHP files directly inside the plugin directory for headers.
        let mut php_files: Vec<String> = Vec::new();
        for entry in fs::read_dir(root).map_err(io_error(root))? {
            let entry = entry.map_err(io_error(root))?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if has_php_extension(&name) && is_plain_file(&entry.path()) {
                php_files.push(name);
            }
        }
        php_files.sort();

        let mut candidates: Vec<(String, BTreeMap<String, String>)> = Vec::new();
        for file in php_files {
            let path = root.join(&file);
            let headers = headers::read(&path, headers::PLUGIN_HEADERS).map_err(io_error(&path))?;
            if headers.contains_key("Plugin Name") {
                candidates.push((file, headers));
            }
        }
        if candidates.is_empty() {
            return Ok(None);
        }

        let mut problems: Vec<String> = Vec::new();
        let files = || candidates.iter().map(|c| c.0.as_str()).collect::<Vec<_>>().join(", ");
        let chosen = if let Some(main_plugin_file) = main_plugin_file {
            let Some(found) = candidates.iter().find(|c| c.0 == main_plugin_file) else {
                problems.push(format!(
                    "Configured main plugin file \"{main_plugin_file}\" has no \"Plugin Name:\" header."
                ));
                return Ok(Some(DetectedProduct {
                    problems,
                    ..DetectedProduct::new(ProductType::Plugin)
                }));
            };
            found
        } else if candidates.len() == 1 {
            &candidates[0]
        } else if let Some(found) = slug.and_then(|slug| candidates.iter().find(|c| c.0 == format!("{slug}.php"))) {
            problems.push(format!(
                "Multiple files declare \"Plugin Name:\" ({}); using {}.",
                files(),
                found.0
            ));
            found
        } else {
            problems.push(format!(
                "Multiple files declare \"Plugin Name:\" ({}). Set the main plugin file in the product settings.",
                files()
            ));
            return Ok(Some(DetectedProduct {
                problems,
                ..DetectedProduct::new(ProductType::Plugin)
            }));
        };

        let (readme, readme_file) = read_readme(root)?;
        let (file, headers) = chosen.clone();
        Ok(Some(DetectedProduct {
            name: headers.get("Plugin Name").cloned(),
            version: headers.get("Version").cloned(),
            text_domain: headers.get("Text Domain").cloned(),
            main_file: Some(file),
            headers,
            readme,
            readme_file,
            problems,
            ..DetectedProduct::new(ProductType::Plugin)
        }))
    }

    // MARK: Theme

    pub(crate) fn detect_theme(&self, root: &Path) -> Result<Option<DetectedProduct>, ProductDetectionError> {
        let style = root.join("style.css");
        if !is_plain_file(&style) {
            return Ok(None);
        }
        let headers = headers::read(&style, headers::THEME_HEADERS).map_err(io_error(&style))?;
        if !headers.contains_key("Theme Name") {
            return Ok(None);
        }
        let (readme, readme_file) = read_readme(root)?;
        let screenshot = ["screenshot.png", "screenshot.jpg", "screenshot.jpeg", "screenshot.gif"]
            .into_iter()
            .find(|name| root.join(name).exists());
        Ok(Some(DetectedProduct {
            name: headers.get("Theme Name").cloned(),
            version: headers.get("Version").cloned(),
            text_domain: headers.get("Text Domain").cloned(),
            main_file: Some("style.css".into()),
            headers,
            readme,
            readme_file,
            screenshot_file: screenshot.map(str::to_string),
            screenshot_size: screenshot.and_then(|name| PixelSize::of_image_at(&root.join(name))),
            ..DetectedProduct::new(ProductType::Theme)
        }))
    }

    /// `readme.txt` (preferred) or `readme.md`, matched case-insensitively at the root.
    /// Returns the file name. Names are compared in sorted order, so the choice between
    /// `README.txt` and `readme.txt` on a case-sensitive file system is deterministic.
    pub fn find_readme(root: &Path) -> Option<String> {
        let mut names: Vec<String> = fs::read_dir(root)
            .ok()?
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        for wanted in ["readme.txt", "readme.md"] {
            if let Some(name) = names.iter().find(|name| name.to_lowercase() == wanted) {
                if is_plain_file(&root.join(name)) {
                    return Some(name.clone());
                }
            }
        }
        None
    }
}

fn read_readme(root: &Path) -> Result<(Option<WordPressReadme>, Option<String>), ProductDetectionError> {
    let Some(name) = ProductDetector::find_readme(root) else {
        return Ok((None, None));
    };
    let path = root.join(&name);
    let readme = readme::parse_file(&path).map_err(io_error(&path))?;
    Ok((Some(readme), Some(name)))
}
