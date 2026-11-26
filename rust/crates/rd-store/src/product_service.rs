//! Port of `Services/ProductService.swift`.

use crate::app_paths::AppPaths;
use crate::store::{ProductRepository, StoreError};
use rd_model::{Id, Product, ProductType, ReleaseMode, SwiftDate};
use rd_support::path_safety;
use rd_versioning::{ProductDetectionError, ProductDetector};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ProductField {
    Name,
    Slug,
    HeaderName,
    SvnUrl,
    WorkingCopy,
    DevelopmentPath,
    AssetsPath,
    GithubUrl,
    MainPluginFile,
    SvnUsername,
    Automation,
}

impl ProductField {
    /// Swift's `ProductValidationIssue.Field.rawValue`.
    pub fn raw_value(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Slug => "slug",
            Self::HeaderName => "headerName",
            Self::SvnUrl => "svnURL",
            Self::WorkingCopy => "workingCopy",
            Self::DevelopmentPath => "developmentPath",
            Self::AssetsPath => "assetsPath",
            Self::GithubUrl => "githubURL",
            Self::MainPluginFile => "mainPluginFile",
            Self::SvnUsername => "svnUsername",
            Self::Automation => "automation",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ProductValidationIssue {
    pub field: ProductField,
    pub message: String,
    pub is_error: bool,
}

impl ProductValidationIssue {
    pub fn new(field: ProductField, message: impl Into<String>, is_error: bool) -> Self {
        Self {
            field,
            message: message.into(),
            is_error,
        }
    }

    pub fn id(&self) -> String {
        format!("{}:{}", self.field.raw_value(), self.message)
    }
}

/// What a development folder says about itself, used to pre-fill product registration.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ProductSuggestion {
    pub product_type: ProductType,
    pub name: String,
    pub slug: String,
    pub version: Option<String>,
    pub main_file: Option<String>,
}

pub trait ProductServicing: Send + Sync {
    fn all_products(&self) -> Result<Vec<Product>, ProductServiceError>;
    fn validate(&self, product: &Product) -> Vec<ProductValidationIssue>;
    fn save(&self, product: &Product) -> Result<(), ProductServiceError>;
    fn delete(&self, product_id: Id) -> Result<(), ProductServiceError>;
    fn suggestion(&self, folder: &Path) -> Result<ProductSuggestion, ProductServiceError>;
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ProductServiceError {
    #[error("{}", error_messages(.0))]
    Invalid(Vec<ProductValidationIssue>),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Detection(#[from] ProductDetectionError),
}

fn error_messages(issues: &[ProductValidationIssue]) -> String {
    issues
        .iter()
        .filter(|i| i.is_error)
        .map(|i| i.message.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

pub struct ProductService {
    repository: Arc<dyn ProductRepository>,
    paths: AppPaths,
    detector: ProductDetector,
}

impl ProductService {
    pub fn new(repository: Arc<dyn ProductRepository>, paths: AppPaths) -> Self {
        Self::with_detector(repository, paths, ProductDetector::new())
    }

    pub fn with_detector(repository: Arc<dyn ProductRepository>, paths: AppPaths, detector: ProductDetector) -> Self {
        Self {
            repository,
            paths,
            detector,
        }
    }

    /// Default working-copy location for a new product.
    pub fn default_working_copy_path(&self, slug: &str, product_type: ProductType) -> String {
        self.paths
            .default_working_copy(slug, product_type)
            .to_string_lossy()
            .into_owned()
    }
}

impl ProductServicing for ProductService {
    fn all_products(&self) -> Result<Vec<Product>, ProductServiceError> {
        Ok(self.repository.all_products()?)
    }

    fn validate(&self, product: &Product) -> Vec<ProductValidationIssue> {
        let mut issues = Vec::new();
        macro_rules! error {
            ($field:expr, $message:expr) => {
                issues.push(ProductValidationIssue::new($field, $message, true))
            };
        }
        macro_rules! warning {
            ($field:expr, $message:expr) => {
                issues.push(ProductValidationIssue::new($field, $message, false))
            };
        }

        if trim_whitespaces(&product.name).is_empty() {
            error!(ProductField::Name, "Name is required.");
        }
        if !path_safety::is_valid_slug(&product.slug) {
            error!(
                ProductField::Slug,
                "Slug must use lowercase letters, digits and hyphens."
            );
        }
        if trim_whitespaces(&product.expected_header_name).is_empty() {
            let kind = match product.product_type {
                ProductType::Plugin => "Plugin",
                ProductType::Theme => "Theme",
            };
            error!(
                ProductField::HeaderName,
                format!("Expected {kind} Name is required for identity checks.")
            );
        }

        // SVN URL
        let url = UrlParts::parse(&product.svn_repository_url);
        match url.scheme.as_deref().map(str::to_lowercase).as_deref() {
            Some("https") => {
                if url.user.is_some() || url.password.is_some() {
                    error!(ProductField::SvnUrl, "The SVN URL must not contain credentials.");
                }
                if let Some(host) = url.host.as_deref().map(str::to_lowercase) {
                    if host.ends_with(".svn.wordpress.org") {
                        let expected_host = match product.product_type {
                            ProductType::Plugin => "plugins.svn.wordpress.org",
                            ProductType::Theme => "themes.svn.wordpress.org",
                        };
                        if host != expected_host {
                            error!(
                                ProductField::SvnUrl,
                                format!(
                                    "{}s are hosted on {expected_host}.",
                                    product.product_type.display_name()
                                )
                            );
                        }
                        if url.path.trim_matches('/') != product.slug {
                            error!(
                                ProductField::SvnUrl,
                                format!(
                                    "The WordPress.org URL must be https://{expected_host}/{}.",
                                    product.slug
                                )
                            );
                        }
                    }
                }
            }
            Some("file") => warning!(ProductField::SvnUrl, "Local file:// repositories are for testing only."),
            _ => error!(ProductField::SvnUrl, "The SVN URL must use https."),
        }

        // Paths
        let wc = &product.working_copy_path;
        if wc.is_empty() || !Path::new(wc).is_absolute() {
            error!(ProductField::WorkingCopy, "The working copy path must be absolute.");
        } else {
            let wc_path = file_url_path(wc);
            let is_home = dirs::home_dir().is_some_and(|home| standardized(&home) == wc_path);
            if wc_path.parent().is_none() || is_home {
                error!(
                    ProductField::WorkingCopy,
                    "Choose a dedicated folder for the SVN working copy."
                );
            }
            if path_safety::is_contained(&wc_path, &self.paths.staging()) {
                error!(
                    ProductField::WorkingCopy,
                    "The working copy cannot live inside the staging area."
                );
            }
            if let Some(dev) = &product.local_development_path {
                let dev_path = file_url_path(dev);
                if path_safety::is_contained(&wc_path, &dev_path) || path_safety::is_contained(&dev_path, &wc_path) {
                    error!(
                        ProductField::WorkingCopy,
                        "The SVN working copy and the development folder must not contain each other."
                    );
                }
            }
            if fs::metadata(&wc_path).is_ok_and(|m| !m.is_dir()) {
                error!(ProductField::WorkingCopy, "The working copy path is a file.");
            }
        }
        if let Some(dev) = product.local_development_path.as_deref().filter(|d| !d.is_empty()) {
            if !is_directory(dev) {
                warning!(ProductField::DevelopmentPath, "The development folder does not exist.");
            }
        }
        if let Some(assets) = product.assets_path.as_deref().filter(|a| !a.is_empty()) {
            if product.product_type == ProductType::Theme {
                error!(ProductField::AssetsPath, "Assets sync applies to plugins only.");
            }
            if !is_directory(assets) {
                error!(ProductField::AssetsPath, "The assets folder does not exist.");
            }
        }
        if let Some(main) = product.main_plugin_file.as_deref().filter(|m| !m.is_empty()) {
            if main.contains('/') || !main.to_lowercase().ends_with(".php") {
                error!(
                    ProductField::MainPluginFile,
                    "The main plugin file must be a .php file name at the plugin root."
                );
            }
        }
        if let Some(github) = &product.github_repository_url {
            if !is_github_repository(github) {
                error!(
                    ProductField::GithubUrl,
                    "Enter a GitHub repository like https://github.com/owner/repo."
                );
            }
        }
        if let Some(username) = &product.svn_username {
            if username != trim_whitespaces(username) {
                error!(
                    ProductField::SvnUsername,
                    "The SVN username has leading or trailing spaces."
                );
            }
        }
        if product.release_mode == ReleaseMode::Automatic && !product.automation_enabled {
            warning!(
                ProductField::Automation,
                "Automatic Release mode has no effect until automation is explicitly enabled."
            );
        }

        issues
    }

    fn save(&self, product: &Product) -> Result<(), ProductServiceError> {
        let issues = self.validate(product);
        if issues.iter().any(|i| i.is_error) {
            return Err(ProductServiceError::Invalid(issues));
        }
        let mut updated = product.clone();
        updated.updated_at = SwiftDate::now();
        Ok(self.repository.save_product(&updated)?)
    }

    fn delete(&self, product_id: Id) -> Result<(), ProductServiceError> {
        Ok(self.repository.delete_product(product_id)?)
    }

    fn suggestion(&self, folder: &Path) -> Result<ProductSuggestion, ProductServiceError> {
        let last_component = folder
            .file_name()
            .map_or_else(|| folder.to_string_lossy(), |n| n.to_string_lossy())
            .into_owned();
        let folder_slug = last_component.to_lowercase();
        let detected = self.detector.detect(folder, None, None, Some(&folder_slug))?;
        let slug = match detected.text_domain.as_deref() {
            Some(domain) if path_safety::is_valid_slug(domain) => domain.to_owned(),
            _ => folder_slug,
        };
        Ok(ProductSuggestion {
            product_type: detected.product_type,
            name: detected.name.unwrap_or(last_component),
            slug,
            version: detected.version,
            main_file: detected.main_file,
        })
    }
}

/// `CharacterSet.whitespaces`: tab and the Unicode space separators (Zs), not line breaks.
fn is_swift_whitespace(c: char) -> bool {
    c == '\t'
        || (c.is_whitespace() && !matches!(c, '\n' | '\u{B}' | '\u{C}' | '\r' | '\u{85}' | '\u{2028}' | '\u{2029}'))
}

/// `trimmingCharacters(in: .whitespaces)`.
fn trim_whitespaces(text: &str) -> &str {
    text.trim_matches(is_swift_whitespace)
}

fn is_directory(path: &str) -> bool {
    fs::metadata(path).is_ok_and(|m| m.is_dir())
}

/// `URL(fileURLWithPath:).standardizedFileURL.path`: relative paths resolve against the
/// current directory; `.` and `..` are removed lexically.
fn file_url_path(path: &str) -> PathBuf {
    let path = Path::new(path);
    if path.is_absolute() {
        standardized(path)
    } else {
        standardized(&std::env::current_dir().unwrap_or_default().join(path))
    }
}

fn standardized(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                result.pop();
            }
            other => result.push(other),
        }
    }
    result
}

/// The parts of a URL that validation looks at, as Foundation's `URL` reports them.
#[derive(Debug, Default, PartialEq)]
struct UrlParts {
    scheme: Option<String>,
    user: Option<String>,
    password: Option<String>,
    host: Option<String>,
    /// Percent-decoded.
    path: String,
}

impl UrlParts {
    /// RFC 3986: `scheme ":" ["//" [userinfo "@"] host [":" port]] path ["?" query] ["#" fragment]`.
    fn parse(text: &str) -> Self {
        let mut parts = Self::default();
        let mut rest = text;
        if let Some((scheme, after)) = text.split_once(':') {
            let valid = scheme.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
                && scheme
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
            if valid {
                parts.scheme = Some(scheme.to_owned());
                rest = after;
            }
        }
        let rest = rest.split(['?', '#']).next().unwrap_or_default();
        let path = match rest.strip_prefix("//") {
            Some(after) => {
                let end = after.find('/').unwrap_or(after.len());
                let (authority, path) = after.split_at(end);
                let host_port = match authority.rsplit_once('@') {
                    Some((userinfo, host_port)) => {
                        match userinfo.split_once(':') {
                            Some((user, password)) => {
                                parts.user = Some(user.to_owned());
                                parts.password = Some(password.to_owned());
                            }
                            None => parts.user = Some(userinfo.to_owned()),
                        }
                        host_port
                    }
                    None => authority,
                };
                let host = if host_port.starts_with('[') {
                    host_port.split_inclusive(']').next().unwrap_or_default()
                } else {
                    host_port.split(':').next().unwrap_or_default()
                };
                if !host.is_empty() {
                    parts.host = Some(host.to_owned());
                }
                path
            }
            None => rest,
        };
        parts.path = percent_decode(path);
        parts
    }
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if let Some(byte) = text
                .get(index + 1..index + 3)
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
            {
                out.push(byte);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `GitHubRepository(string:) != nil` from `GitHub/GitHubService.swift`: accepts `owner/name`,
/// `https://github.com/owner/name(.git)` and `git@github.com:owner/name.git`.
fn is_github_repository(text: &str) -> bool {
    let mut s = text.trim();
    for prefix in [
        "https://github.com/",
        "http://github.com/",
        "git@github.com:",
        "github.com/",
    ] {
        if s.to_lowercase().starts_with(prefix) {
            s = s.get(prefix.len()..).unwrap_or_default();
            break;
        }
    }
    let s = s.strip_suffix(".git").unwrap_or(s);
    let s = s.strip_suffix('/').unwrap_or(s);
    // Swift's `split` drops empty pieces.
    let parts: Vec<&str> = s.split('/').filter(|p| !p.is_empty()).collect();
    parts.len() == 2
        && parts.iter().all(|part| {
            *part != "."
                && *part != ".."
                && part
                    .chars()
                    .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.'))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_url_parts_like_foundation() {
        let url = UrlParts::parse("https://user:pw@plugins.svn.wordpress.org/sample-plugin/");
        assert_eq!(url.scheme.as_deref(), Some("https"));
        assert_eq!(url.user.as_deref(), Some("user"));
        assert_eq!(url.password.as_deref(), Some("pw"));
        assert_eq!(url.host.as_deref(), Some("plugins.svn.wordpress.org"));
        assert_eq!(url.path, "/sample-plugin/");
        let file = UrlParts::parse("file:///tmp/repo");
        assert_eq!(file.scheme.as_deref(), Some("file"));
        assert_eq!(file.host, None);
        assert_eq!(file.path, "/tmp/repo");
        assert_eq!(UrlParts::parse("plugins.svn.wordpress.org/x").scheme, None);
        assert_eq!(UrlParts::parse("https://h:8443/a%20b?q#f").path, "/a b");
    }

    #[test]
    fn parses_github_repositories() {
        for good in [
            "owner/repo",
            "https://github.com/owner/repo",
            "https://GitHub.com/owner/repo.git",
            "git@github.com:owner/repo.git",
            "github.com/owner/repo/",
            " owner//repo ",
        ] {
            assert!(is_github_repository(good), "{good}");
        }
        for bad in ["owner", "owner/repo/extra", "https://gitlab.com/o/r", "o/..", "o/r p"] {
            assert!(!is_github_repository(bad), "{bad}");
        }
    }

    #[test]
    fn trims_only_swift_whitespaces() {
        assert_eq!(trim_whitespaces("\t a \u{A0}"), "a");
        assert_eq!(trim_whitespaces("\na\n"), "\na\n");
    }
}
