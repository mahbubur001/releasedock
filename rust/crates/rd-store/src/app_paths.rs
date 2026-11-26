//! Port of `Support/AppPaths.swift`.

use rd_model::{Id, ProductType};
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};

/// Environment variable with an absolute data folder (UI testing and isolated profiles).
pub const DATA_DIR_VARIABLE: &str = "RELEASEDOCK_DATA_DIR";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AppPathsError {
    /// `dirs::data_dir()` found no home or data folder for this user.
    #[error("The application data folder could not be determined.")]
    NoDataDirectory,
}

/// Layout of the ReleaseDock data directory:
///
/// ```text
/// ReleaseDockData/
/// ├── Products/   default SVN working copies, one folder per product
/// ├── Staging/    per-release extraction and clean release candidates
/// ├── Releases/   release manifests and reports
/// ├── Backups/    database backups before migrations, working-copy diffs
/// ├── Logs/       exported diagnostics
/// └── Database/   SQLite database
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    pub root: PathBuf,
}

impl AppPaths {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The default data folder for this process's environment; see [`AppPaths::default_root_with`].
    pub fn default_root() -> Result<PathBuf, AppPathsError> {
        Self::default_root_with(&std::env::vars().collect())
    }

    /// `ReleaseDock/ReleaseDockData` in the platform data folder, or the absolute path in
    /// `RELEASEDOCK_DATA_DIR`:
    ///
    /// - macOS: `~/Library/Application Support/ReleaseDock/ReleaseDockData` (the Swift app's folder)
    /// - Windows: `%APPDATA%\ReleaseDock\ReleaseDockData`
    /// - Linux: `$XDG_DATA_HOME/ReleaseDock/ReleaseDockData` or `~/.local/share/ReleaseDock/ReleaseDockData`
    ///
    /// A relative override is ignored, as in Swift.
    pub fn default_root_with(environment: &HashMap<String, String>) -> Result<PathBuf, AppPathsError> {
        if let Some(path) = environment.get(DATA_DIR_VARIABLE).map(Path::new) {
            if path.is_absolute() {
                return Ok(path.to_path_buf());
            }
        }
        let data = dirs::data_dir().ok_or(AppPathsError::NoDataDirectory)?;
        Ok(data.join("ReleaseDock").join("ReleaseDockData"))
    }

    pub fn products(&self) -> PathBuf {
        self.root.join("Products")
    }

    pub fn staging(&self) -> PathBuf {
        self.root.join("Staging")
    }

    pub fn releases(&self) -> PathBuf {
        self.root.join("Releases")
    }

    pub fn backups(&self) -> PathBuf {
        self.root.join("Backups")
    }

    pub fn logs(&self) -> PathBuf {
        self.root.join("Logs")
    }

    /// One JSON-lines file of svn commands per product.
    pub fn transcripts(&self) -> PathBuf {
        self.root.join("Transcripts")
    }

    pub fn database(&self) -> PathBuf {
        self.root.join("Database")
    }

    pub fn database_file(&self) -> PathBuf {
        self.database().join("releasedock.sqlite")
    }

    pub fn staging_directory(&self, release_id: Id) -> PathBuf {
        self.staging().join(release_id.to_string())
    }

    pub fn release_directory(&self, slug: &str, version: &str, release_id: Id) -> PathBuf {
        let id = release_id.to_string();
        self.releases().join(slug).join(format!("{version}-{}", &id[..8]))
    }

    pub fn default_working_copy(&self, slug: &str, product_type: ProductType) -> PathBuf {
        let kind = match product_type {
            ProductType::Plugin => "plugin",
            ProductType::Theme => "theme",
        };
        self.products().join(format!("{kind}-{slug}")).join("svn")
    }

    /// Creates every directory with owner-only permissions (0700 on Unix; Windows folders
    /// inherit the user profile's ACL).
    pub fn create_all(&self) -> io::Result<()> {
        for dir in [
            self.root.clone(),
            self.products(),
            self.staging(),
            self.releases(),
            self.backups(),
            self.logs(),
            self.database(),
        ] {
            create_private_dir(&dir)?;
        }
        Ok(())
    }
}

#[cfg(unix)]
fn create_private_dir(dir: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)
}

#[cfg(not(unix))]
fn create_private_dir(dir: &Path) -> io::Result<()> {
    std::fs::create_dir_all(dir)
}
