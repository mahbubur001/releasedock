//! Port of `Release/ReleaseStagingService.swift` (`ReleaseStaging`, `ReleaseStagingService`).
//! `StagedCandidate` lives in `rd-records`; `ReleaseError` is in [`crate::error`].

use crate::error::{ReleaseError, io_error};
use crate::fs_util::{create_dirs, path_string, remove_item, write_atomically};
use rd_inspection::{ZipInspecting, ZipInspectionService};
use rd_model::{EntryKind, Id, PackageEntry, PackageInspection};
use rd_records::{ReleaseManifest, StagedCandidate};
use rd_store::AppPaths;
use rd_support::{hashing, path_safety};
use std::fs;
use std::path::{Path, PathBuf};

pub trait ReleaseStaging: Send + Sync {
    fn workspace(&self, release_id: Id) -> Result<PathBuf, ReleaseError>;
    /// Extracts an inspected ZIP into the release workspace and returns the package root.
    fn extract(
        &self,
        zip: &Path,
        inspection: &PackageInspection,
        release_id: Id,
    ) -> Result<(PathBuf, Vec<PackageEntry>), ReleaseError>;
    /// Copies the included files from `source_root` into a fresh candidate directory,
    /// verifying each file against the hash recorded at inspection time.
    fn build_candidate(
        &self,
        source_root: &Path,
        entries: &[PackageEntry],
        release_id: Id,
    ) -> Result<StagedCandidate, ReleaseError>;
    fn write_manifest(
        &self,
        manifest: &ReleaseManifest,
        slug: &str,
        version: &str,
        release_id: Id,
    ) -> Result<PathBuf, ReleaseError>;
    fn write_backup(&self, text: &str, name: &str) -> Result<PathBuf, ReleaseError>;
    fn remove_workspace(&self, release_id: Id);
    /// The workspace path recorded as the release's `stagingPath`. Swift records it only
    /// when the staging is a `ReleaseStagingService` (`deps.staging is ReleaseStagingService`);
    /// other implementations keep this default and record nothing.
    fn recorded_staging_path(&self, _release_id: Id) -> Option<String> {
        None
    }
}

/// Staging in the data folder: `Staging/<release-id>/` (workspace, extracted ZIP,
/// candidate), `Releases/<slug>/<version>-<id>/manifest.json`, and `Backups/`.
#[derive(Clone, Debug)]
pub struct ReleaseStagingService {
    pub paths: AppPaths,
}

impl ReleaseStagingService {
    pub fn new(paths: AppPaths) -> Self {
        Self { paths }
    }
}

impl ReleaseStaging for ReleaseStagingService {
    fn workspace(&self, release_id: Id) -> Result<PathBuf, ReleaseError> {
        let dir = self.paths.staging_directory(release_id);
        create_dirs(&dir, 0o700).map_err(io_error(&dir))?;
        Ok(dir)
    }

    fn extract(
        &self,
        zip: &Path,
        inspection: &PackageInspection,
        release_id: Id,
    ) -> Result<(PathBuf, Vec<PackageEntry>), ReleaseError> {
        let destination = self.workspace(release_id)?.join("extracted");
        if fs::symlink_metadata(&destination).is_ok() {
            remove_item(&destination).map_err(io_error(&destination))?;
        }
        let entries = ZipInspectionService::new().extract(zip, inspection, &destination)?;
        Ok((destination, entries))
    }

    fn build_candidate(
        &self,
        source_root: &Path,
        entries: &[PackageEntry],
        release_id: Id,
    ) -> Result<StagedCandidate, ReleaseError> {
        let candidate = self.workspace(release_id)?.join("candidate");
        if fs::symlink_metadata(&candidate).is_ok() {
            remove_item(&candidate).map_err(io_error(&candidate))?;
        }
        create_dirs(&candidate, 0o755).map_err(io_error(&candidate))?;

        let mut sorted: Vec<&PackageEntry> = entries.iter().collect();
        sorted.sort_by(|a, b| a.path.cmp(&b.path));
        let mut result = Vec::with_capacity(sorted.len());
        for entry in sorted {
            let components = path_safety::validate_relative_path(&entry.path)?;
            let destination = path_safety::resolve(&components, &candidate)?;
            match entry.kind {
                EntryKind::Directory => {
                    fs::create_dir_all(&destination).map_err(io_error(&destination))?;
                    result.push(entry.clone());
                }
                EntryKind::File => {
                    let source = path_safety::resolve(&components, source_root)?;
                    let is_plain_file = fs::symlink_metadata(&source).is_ok_and(|m| m.file_type().is_file());
                    if !is_plain_file {
                        return Err(ReleaseError::CandidateChanged(format!(
                            "{} is no longer a regular file",
                            entry.path
                        )));
                    }
                    if let Some(parent) = destination.parent() {
                        fs::create_dir_all(parent).map_err(io_error(parent))?;
                    }
                    fs::copy(&source, &destination).map_err(io_error(&source))?;
                    let sha = hashing::sha256_file(&destination).map_err(io_error(&destination))?;
                    if entry.sha256.as_ref().is_some_and(|expected| *expected != sha) {
                        return Err(ReleaseError::CandidateChanged(format!(
                            "{} was modified after inspection",
                            entry.path
                        )));
                    }
                    let mut copied = entry.clone();
                    copied.sha256 = Some(sha);
                    copied.size = fs::metadata(&destination).map(|m| m.len()).unwrap_or(0);
                    result.push(copied);
                }
                EntryKind::Symlink => {
                    return Err(ReleaseError::CandidateChanged(format!(
                        "{} is a symbolic link",
                        entry.path
                    )));
                }
            }
        }
        let digest = hashing::tree_digest(
            result
                .iter()
                .filter(|e| e.kind == EntryKind::File)
                .map(|e| (e.path.as_str(), e.sha256.as_deref().unwrap_or(""))),
        );
        Ok(StagedCandidate::new(path_string(&candidate), result, digest))
    }

    fn write_manifest(
        &self,
        manifest: &ReleaseManifest,
        slug: &str,
        version: &str,
        release_id: Id,
    ) -> Result<PathBuf, ReleaseError> {
        let dir = self.paths.release_directory(slug, version, release_id);
        create_dirs(&dir, 0o700).map_err(io_error(&dir))?;
        let url = dir.join("manifest.json");
        write_atomically(&url, &manifest.encoded()?).map_err(io_error(&url))?;
        Ok(url)
    }

    fn write_backup(&self, text: &str, name: &str) -> Result<PathBuf, ReleaseError> {
        let backups = self.paths.backups();
        create_dirs(&backups, 0o700).map_err(io_error(&backups))?;
        let url = backups.join(name);
        write_atomically(&url, text.as_bytes()).map_err(io_error(&url))?;
        Ok(url)
    }

    fn remove_workspace(&self, release_id: Id) {
        let _ = remove_item(&self.paths.staging_directory(release_id));
    }

    fn recorded_staging_path(&self, release_id: Id) -> Option<String> {
        self.workspace(release_id).ok().map(|p| path_string(&p))
    }
}
