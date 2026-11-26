//! Port of `Services/RepositorySetupService.swift`.

use crate::repository_layout::{self, LayoutError, is_theme_version_directory};
use crate::url_text::{deleting_last_path_component, host, last_path_component, trimming_trailing_slash};
use rd_model::{Product, ProductType};
use rd_svn::{SvnError, SvnErrorKind, SvnInfo, SvnNodeKind, SvnServicing};
use rd_versioning::{ProductDetector, VersionService, VersionServicing, headers};
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use url::Url;

/// What ReleaseDock learned about an SVN repository for a product.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RepositoryDescription {
    /// The repository URL as text (Swift `URL.absoluteString`), without a trailing `/`.
    pub url: String,
    pub slug: String,
    /// Inferred from the WordPress.org host or the repository layout; None when unclear.
    pub product_type: Option<ProductType>,
    pub product_name: Option<String>,
    pub current_version: Option<String>,
    pub versions: Vec<String>,
    pub revision: Option<i64>,
    pub notes: Vec<String>,
}

impl RepositoryDescription {
    pub fn new(url: impl Into<String>, slug: impl Into<String>, product_type: Option<ProductType>) -> Self {
        Self {
            url: url.into(),
            slug: slug.into(),
            product_type,
            product_name: None,
            current_version: None,
            versions: Vec::new(),
            revision: None,
            notes: Vec::new(),
        }
    }
}

/// An existing checkout selected by the user.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct WorkingCopyDescription {
    pub path: PathBuf,
    pub repository: RepositoryDescription,
    pub revision: i64,
    pub local_changes: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum RepositorySetupError {
    #[error("“{0}” is not a valid https SVN URL.")]
    InvalidUrl(String),
    #[error("No repository exists at {0}. Check the slug and product type.")]
    RepositoryNotFound(String),
    #[error("{found} is inside the product repository. Use the product root instead: {suggestion}")]
    NotProductRoot { found: String, suggestion: String },
    #[error("{0} is not an SVN working copy.")]
    NotAWorkingCopy(String),
    #[error("{selected} is part of a larger working copy. Select its root: {root}")]
    NotWorkingCopyRoot { selected: String, root: String },
    #[error("{0} does not look like a WordPress.org plugin (trunk/tags) or theme (version folders) repository.")]
    UnknownLayout(String),
    #[error("{0} already exists and is not empty. Choose an empty or new folder.")]
    DestinationNotEmpty(String),
    /// An svn command failed (Swift rethrows `SVNError`).
    #[error(transparent)]
    Svn(#[from] SvnError),
    /// The local folder could not be read or created.
    #[error("{0}")]
    Io(String),
}

impl PartialEq for RepositorySetupError {
    /// Swift's `Equatable` cases compare by value; wrapped svn and I/O errors compare by message.
    fn eq(&self, other: &Self) -> bool {
        use RepositorySetupError::*;
        match (self, other) {
            (InvalidUrl(a), InvalidUrl(b))
            | (RepositoryNotFound(a), RepositoryNotFound(b))
            | (NotAWorkingCopy(a), NotAWorkingCopy(b))
            | (UnknownLayout(a), UnknownLayout(b))
            | (DestinationNotEmpty(a), DestinationNotEmpty(b))
            | (Io(a), Io(b)) => a == b,
            (
                NotProductRoot {
                    found: a,
                    suggestion: b,
                },
                NotProductRoot {
                    found: c,
                    suggestion: d,
                },
            ) => a == c && b == d,
            (NotWorkingCopyRoot { selected: a, root: b }, NotWorkingCopyRoot { selected: c, root: d }) => {
                a == c && b == d
            }
            (Svn(a), Svn(b)) => a.to_string() == b.to_string(),
            _ => false,
        }
    }
}

impl From<LayoutError> for RepositorySetupError {
    fn from(error: LayoutError) -> Self {
        match error {
            LayoutError::Svn(error) => Self::Svn(error),
            LayoutError::Io(message) => Self::Io(message),
        }
    }
}

/// Connects a product to SVN: inspects a repository on the server and checks it out
/// sparsely, or adopts an existing working copy chosen by the user.
#[derive(Clone)]
pub struct RepositorySetupService {
    svn: Arc<dyn SvnServicing>,
    detector: ProductDetector,
}

impl RepositorySetupService {
    pub fn new(svn: Arc<dyn SvnServicing>) -> Self {
        Self {
            svn,
            detector: ProductDetector::new(),
        }
    }

    // MARK: Remote

    /// Normalizes user input: a full URL, or a slug combined with the WordPress.org host for `product_type`.
    pub fn repository_url(input: &str, product_type: ProductType) -> Result<String, RepositorySetupError> {
        let trimmed = input.trim();
        if !trimmed.contains('/') && rd_support::path_safety::is_valid_slug(trimmed) {
            return Ok(product_type.default_repository_url(trimmed));
        }
        let invalid = || RepositorySetupError::InvalidUrl(trimmed.to_owned());
        let mut url = Url::parse(trimmed).map_err(|_| invalid())?;
        if !["https", "file"].contains(&url.scheme()) || !url.username().is_empty() || url.password().is_some() {
            return Err(invalid());
        }
        let path = url.path().trim_end_matches('/').to_owned();
        url.set_path(&path);
        let text = url.to_string();
        let text = trimming_trailing_slash(&text).to_owned();
        if last_path_component(&text).is_empty() {
            return Err(invalid());
        }
        Ok(text)
    }

    pub fn inferred_type(host: Option<&str>) -> Option<ProductType> {
        match host.map(str::to_lowercase).as_deref() {
            Some("plugins.svn.wordpress.org") => Some(ProductType::Plugin),
            Some("themes.svn.wordpress.org") => Some(ProductType::Theme),
            _ => None,
        }
    }

    /// Reads the repository on the server without changing anything locally.
    pub async fn describe_remote(
        &self,
        url: &str,
        expected_type: Option<ProductType>,
    ) -> Result<RepositoryDescription, RepositorySetupError> {
        let last = last_path_component(url);
        let parent = deleting_last_path_component(url);
        let parent_last = last_path_component(&parent);
        if matches!(last.as_str(), "trunk" | "tags" | "assets") || parent_last == "tags" {
            let root = if matches!(last.as_str(), "trunk" | "tags" | "assets") {
                parent
            } else {
                deleting_last_path_component(&parent)
            };
            return Err(RepositorySetupError::NotProductRoot {
                found: url.to_owned(),
                suggestion: trimming_trailing_slash(&root).to_owned(),
            });
        }

        // On WordPress.org the host decides the type; elsewhere the layout does.
        let host_type = Self::inferred_type(host(url).as_deref());
        let mut product_type = host_type;
        let mut revision = None;
        if host_type.is_none() {
            let (info, list) = tokio::join!(self.svn.info(url), self.svn.list(url, false));
            let info = match info {
                Ok(info) => info,
                Err(error) if error.kind() == Some(SvnErrorKind::NotFound) => {
                    return Err(RepositorySetupError::RepositoryNotFound(url.to_owned()));
                }
                Err(error) => return Err(error.into()),
            };
            revision = Some(info.last_changed_revision.unwrap_or(info.revision));
            let entries = list?;
            let directories: HashSet<&str> = entries
                .iter()
                .filter(|e| e.kind == SvnNodeKind::Dir)
                .map(|e| e.name.as_str())
                .collect();
            let layout_type = if directories.contains("trunk") && directories.contains("tags") {
                Some(ProductType::Plugin)
            } else if directories.iter().any(|d| is_theme_version_directory(d)) {
                Some(ProductType::Theme)
            } else {
                None
            };
            product_type = layout_type.or(expected_type);
            if product_type.is_none() {
                if entries.is_empty() {
                    let mut description = RepositoryDescription::new(url, last_path_component(url), None);
                    description.revision = revision;
                    description.notes = vec!["The repository is empty; choose Plugin or Theme.".into()];
                    return Ok(description);
                }
                return Err(RepositorySetupError::UnknownLayout(url.to_owned()));
            }
        }
        let Some(product_type) = product_type else {
            return Err(RepositorySetupError::UnknownLayout(url.to_owned()));
        };

        let slug = last_path_component(url);
        let probe = Product::new(&slug, &slug, product_type, url, "/");
        // The main plugin file is usually <slug>.php; read it alongside the repository scan.
        let header_name = async {
            match product_type {
                ProductType::Plugin => self.plugin_header_name(url, &slug).await,
                ProductType::Theme => Ok(None),
            }
        };
        let inspect = repository_layout::inspect_remote(&probe, None, self.svn.as_ref());
        let (header_name, snapshot) = tokio::join!(header_name, inspect);
        let snapshot = snapshot?;
        if snapshot.repository_exists == Some(false) {
            return Err(RepositorySetupError::RepositoryNotFound(url.to_owned()));
        }

        let mut description = RepositoryDescription::new(url, slug, Some(product_type));
        description.revision = snapshot.remote_revision.or(revision);
        description.current_version = snapshot.current_version.clone();
        description.versions = snapshot.existing_versions.clone();
        description.notes = snapshot
            .structure_problems
            .iter()
            .chain(&snapshot.notes)
            .cloned()
            .collect();
        description.product_name = match product_type {
            ProductType::Plugin => header_name?.or(snapshot.readme_title),
            ProductType::Theme => self.theme_name(url, snapshot.current_version.as_deref()).await?,
        };
        Ok(description)
    }

    async fn plugin_header_name(&self, url: &str, slug: &str) -> Result<Option<String>, SvnError> {
        let Some(data) =
            repository_layout::cat_if_exists(&format!("{url}/trunk/{slug}.php"), self.svn.as_ref()).await?
        else {
            return Ok(None);
        };
        Ok(Self::header(&data, "Plugin Name"))
    }

    async fn theme_name(&self, url: &str, current_version: Option<&str>) -> Result<Option<String>, SvnError> {
        let Some(current_version) = current_version else {
            return Ok(None);
        };
        let Some(data) =
            repository_layout::cat_if_exists(&format!("{url}/{current_version}/style.css"), self.svn.as_ref()).await?
        else {
            return Ok(None);
        };
        Ok(Self::header(&data, "Theme Name"))
    }

    fn header(data: &[u8], name: &str) -> Option<String> {
        let head = &data[..data.len().min(headers::READ_LIMIT)];
        headers::parse(&String::from_utf8_lossy(head), &[name]).remove(name)
    }

    /// Sparse checkout into an empty (or new) folder, laid out exactly as releases need it.
    pub async fn checkout(
        &self,
        description: &RepositoryDescription,
        product_type: ProductType,
        destination: &Path,
    ) -> Result<(), RepositorySetupError> {
        if destination.exists() {
            let entries = std::fs::read_dir(destination)
                .map_err(|e| RepositorySetupError::Io(format!("{}: {e}", destination.display())))?;
            let busy = entries
                .filter_map(Result::ok)
                .any(|entry| entry.file_name() != ".DS_Store");
            if busy {
                return Err(RepositorySetupError::DestinationNotEmpty(
                    destination.to_string_lossy().into_owned(),
                ));
            }
        }
        let product = Product::new(
            &description.slug,
            &description.slug,
            product_type,
            &description.url,
            &destination.to_string_lossy(),
        );
        repository_layout::prepare_working_copy(&product, description.current_version.as_deref(), self.svn.as_ref())
            .await?;
        Ok(())
    }

    // MARK: Existing working copy

    /// Describes a user-selected checkout from local data only (fast, no network): the
    /// repository URL from `svn info`, layout, versions and headers from disk, local changes.
    pub async fn describe_working_copy_locally(
        &self,
        path: &Path,
    ) -> Result<WorkingCopyDescription, RepositorySetupError> {
        let display = path.to_string_lossy().into_owned();
        let info: SvnInfo = match self.svn.info(&display).await {
            Ok(info) => info,
            Err(error)
                if matches!(
                    error.kind(),
                    Some(SvnErrorKind::NotAWorkingCopy | SvnErrorKind::NotFound)
                ) =>
            {
                return Err(RepositorySetupError::NotAWorkingCopy(display));
            }
            Err(error) => return Err(error.into()),
        };
        if let Some(root) = &info.working_copy_root {
            if !same_path(Path::new(root), path) {
                return Err(RepositorySetupError::NotWorkingCopyRoot {
                    selected: display,
                    root: root.clone(),
                });
            }
        }
        if Url::parse(&info.url).is_err() {
            return Err(RepositorySetupError::InvalidUrl(info.url.clone()));
        }
        let url = info.url.clone();
        let last = last_path_component(&url);
        if matches!(last.as_str(), "trunk" | "tags" | "assets") {
            return Err(RepositorySetupError::NotProductRoot {
                suggestion: trimming_trailing_slash(&deleting_last_path_component(&url)).to_owned(),
                found: url,
            });
        }
        let mut repository = self.describe_locally(&url, path);
        repository.revision = Some(info.revision);
        let local_changes = repository_layout::local_changes(path, self.svn.as_ref()).await?;
        Ok(WorkingCopyDescription {
            path: path.to_path_buf(),
            repository,
            revision: info.revision,
            local_changes,
        })
    }

    /// Local description refreshed with server details when the server is reachable.
    pub async fn describe_working_copy(&self, path: &Path) -> Result<WorkingCopyDescription, RepositorySetupError> {
        let mut description = self.describe_working_copy_locally(path).await?;
        match self
            .describe_remote(&description.repository.url, description.repository.product_type)
            .await
        {
            Ok(remote) => description.repository = Self::merge(&description.repository, remote),
            Err(RepositorySetupError::Svn(error))
                if matches!(error.kind(), Some(SvnErrorKind::Network | SvnErrorKind::Certificate)) =>
            {
                description.repository.notes.push(format!(
                    "The server could not be reached ({error}); details come from the local copy."
                ));
            }
            Err(error) => return Err(error),
        }
        Ok(description)
    }

    /// Server data wins for versions; the local name is kept if the server could not supply one.
    pub fn merge(local: &RepositoryDescription, remote: RepositoryDescription) -> RepositoryDescription {
        let mut merged = remote;
        merged.product_name = merged.product_name.or_else(|| local.product_name.clone());
        merged.product_type = merged.product_type.or(local.product_type);
        merged
    }

    /// Builds a fully populated product from a repository and its local checkout. Identity
    /// comes from the checked-out headers (trunk for plugins, the latest version folder for
    /// themes), falling back to what the server reported.
    pub fn make_product(
        &self,
        description: &RepositoryDescription,
        product_type: ProductType,
        working_copy: &Path,
    ) -> Product {
        let product_root = match product_type {
            ProductType::Plugin => Some(working_copy.join("trunk")),
            ProductType::Theme => description.current_version.as_ref().map(|v| working_copy.join(v)),
        };
        let detected = product_root.and_then(|root| {
            self.detector
                .detect(&root, Some(product_type), None, Some(&description.slug))
                .ok()
        });
        let name = detected
            .as_ref()
            .and_then(|d| d.name.clone())
            .or_else(|| description.product_name.clone())
            .unwrap_or_else(|| description.slug.clone());
        let mut product = Product::new(
            &name,
            &description.slug,
            product_type,
            &description.url,
            &working_copy.to_string_lossy(),
        );
        product.expected_header_name = name;
        product.current_version = description.current_version.clone();
        if product_type == ProductType::Plugin {
            if let Some(main) = detected.and_then(|d| d.main_file) {
                if main != format!("{}.php", description.slug) {
                    product.main_plugin_file = Some(main);
                }
            }
        }
        product
    }

    fn describe_locally(&self, url: &str, path: &Path) -> RepositoryDescription {
        let names = directory_names(path);
        let mut product_type = Self::inferred_type(host(url).as_deref());
        if product_type.is_none() {
            product_type = if names.iter().any(|n| n == "trunk") {
                Some(ProductType::Plugin)
            } else if names.iter().any(|n| is_theme_version_directory(n)) {
                Some(ProductType::Theme)
            } else {
                None
            };
        }
        let versions: Vec<String> = if product_type == Some(ProductType::Plugin) {
            directory_names(&path.join("tags"))
        } else {
            names.into_iter().filter(|n| is_theme_version_directory(n)).collect()
        }
        .into_iter()
        .filter(|n| !n.starts_with('.'))
        .collect();
        let current = VersionService::new().highest(&versions);
        let product_root = if product_type == Some(ProductType::Plugin) {
            Some(path.join("trunk"))
        } else {
            current.as_ref().map(|v| path.join(v))
        };
        let slug = last_path_component(url);
        let name = product_root.and_then(|root| {
            self.detector
                .detect(&root, product_type, None, Some(&slug))
                .ok()
                .and_then(|d| d.name)
        });
        let mut description = RepositoryDescription::new(url, slug, product_type);
        description.product_name = name;
        description.current_version = current;
        description.versions = versions;
        description
    }
}

/// `FileManager.contentsOfDirectory(atPath:)` names (sorted here; Foundation's order is
/// unspecified), or nothing when the folder cannot be read.
fn directory_names(path: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(path)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// `URL.standardizedFileURL.path` equality: `.` and `..` resolved lexically, trailing
/// separators ignored; case-insensitive on Windows, where svn reports `C:/…` paths.
fn same_path(a: &Path, b: &Path) -> bool {
    let (a, b) = (standardized(a), standardized(b));
    if cfg!(windows) {
        a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
    } else {
        a == b
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
