//! `WorkingCopyOperations` and `FileMirror` from `Release/ReleaseStrategy.swift`: the shared
//! working-copy helpers of both strategies. `catIfExists`, `localChanges` and `changes` are
//! in `rd_integrations::repository_layout` and re-exported here.

use crate::error::{ReleaseError, io_error};
use crate::fs_util::{file_exists, file_exists_is_dir, remove_item, standardized};
use rd_model::{EntryKind, PackageEntry};
use rd_records::StagedCandidate;
use rd_support::{hashing, path_safety};
use rd_svn::{SvnItemStatus, SvnNodeKind, SvnServicing};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

pub use rd_integrations::repository_layout::{cat_if_exists, changes, local_changes};

/// Makes `target` contain exactly the candidate files (ignoring `.svn`), then schedules
/// unversioned paths for addition and missing paths for deletion.
pub async fn mirror(candidate: &StagedCandidate, target: &Path, svn: &dyn SvnServicing) -> Result<(), ReleaseError> {
    // Paths that change between file and directory must be scheduled for deletion first;
    // replacing them on disk alone leaves svn with an "obstructed" node.
    let type_changes = file_mirror::type_changes(&candidate.entries, target);
    if !type_changes.is_empty() {
        svn.delete(&type_changes).await?;
    }
    file_mirror::mirror(&candidate.root_dir(), &candidate.entries, target)?;
    if !type_changes.is_empty() {
        svn.add(&type_changes).await?;
    }
    schedule_adds_and_deletes(target, svn).await
}

pub async fn schedule_adds_and_deletes(target: &Path, svn: &dyn SvnServicing) -> Result<(), ReleaseError> {
    let status = svn.status(target, false).await?;
    let obstructed: Vec<String> = status
        .iter()
        .filter(|e| e.item == SvnItemStatus::Obstructed)
        .map(|e| e.path.clone())
        .collect();
    if !obstructed.is_empty() {
        return Err(ReleaseError::WorkingCopyObstructed(obstructed));
    }
    let unversioned: Vec<PathBuf> = status
        .iter()
        .filter(|e| matches!(e.item, SvnItemStatus::Unversioned | SvnItemStatus::Ignored))
        .map(|e| PathBuf::from(&e.path))
        .collect();
    let missing: Vec<PathBuf> = status
        .iter()
        .filter(|e| e.item == SvnItemStatus::Missing)
        .map(|e| PathBuf::from(&e.path))
        .collect();
    svn.add(&unversioned).await?;
    svn.delete(&missing).await?;
    Ok(())
}

/// Files missing from, unexpected on, and differing (SHA-256) on the server.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TreeDifferences {
    pub missing: Vec<String>,
    pub unexpected: Vec<String>,
    pub mismatched: Vec<String>,
}

/// Lists all versioned files under a remote URL and compares them (and their SHA-256
/// after export) with the candidate.
pub async fn verify_remote_tree(
    url: &str,
    candidate: &StagedCandidate,
    svn: &dyn SvnServicing,
    scratch: &Path,
) -> Result<TreeDifferences, ReleaseError> {
    let listing = svn.list(url, true).await?;
    let remote_files: BTreeSet<String> = listing
        .into_iter()
        .filter(|e| e.kind == SvnNodeKind::File)
        .map(|e| e.name)
        .collect();
    let expected: BTreeSet<String> = candidate
        .entries
        .iter()
        .filter(|e| e.kind == EntryKind::File)
        .map(|e| e.path.clone())
        .collect();
    let missing = expected.difference(&remote_files).cloned().collect();
    let unexpected = remote_files.difference(&expected).cloned().collect();

    let export_dir = scratch.join(format!("verify-{}", rd_model::Id::new()));
    let _ = remove_item(&export_dir);
    svn.export(url, &export_dir).await?;
    let mut mismatched = Vec::new();
    for entry in &candidate.entries {
        if entry.kind != EntryKind::File || !remote_files.contains(&entry.path) {
            continue;
        }
        let sha = hashing::sha256_file(&export_dir.join(&entry.path)).ok();
        if sha != entry.sha256 {
            mismatched.push(entry.path.clone());
        }
    }
    let _ = remove_item(&export_dir);
    Ok(TreeDifferences {
        missing,
        unexpected,
        mismatched,
    })
}

/// Returns a mismatch description if `path` is a working copy of a different URL.
pub async fn working_copy_mismatch(
    path: &Path,
    expected: &str,
    svn: &dyn SvnServicing,
) -> Result<Option<String>, ReleaseError> {
    if !file_exists(&path.join(".svn")) {
        let is_empty = fs::read_dir(path).map_or(true, |mut entries| entries.next().is_none());
        return Ok(
            (!is_empty).then(|| format!("{} exists, is not empty and is not an SVN working copy", path.display()))
        );
    }
    let info = svn.info(&path.to_string_lossy()).await?;
    let normalize = |s: &str| s.strip_suffix('/').unwrap_or(s).to_owned();
    if normalize(&info.url) != normalize(expected) {
        return Ok(Some(format!(
            "Working copy at {} is a checkout of {}, not {expected}",
            path.display(),
            info.url
        )));
    }
    Ok(None)
}

/// Candidate files (other than `ignoring`) that are missing from, or differ from, the
/// checked-out `released_tree`, plus released files the candidate no longer has.
pub fn differences(
    candidate: &StagedCandidate,
    released_tree: &Path,
    ignoring: &HashSet<String>,
) -> Result<Vec<String>, ReleaseError> {
    let mut released: BTreeSet<String> = BTreeSet::new();
    collect_regular_files(released_tree, "", &mut released)?;
    let mut differences = Vec::new();
    let files: Vec<&PackageEntry> = candidate
        .entries
        .iter()
        .filter(|e| e.kind == EntryKind::File && !ignoring.contains(&e.path))
        .collect();
    for entry in &files {
        let file = released_tree.join(&entry.path);
        if !released.contains(&entry.path) || hashing::sha256_file(&file).ok() != entry.sha256 {
            differences.push(entry.path.clone());
        }
    }
    let file_paths: HashSet<&str> = files.iter().map(|e| e.path.as_str()).collect();
    differences.extend(
        released
            .iter()
            .filter(|p| !file_paths.contains(p.as_str()) && !ignoring.contains(*p))
            .cloned(),
    );
    differences.sort();
    Ok(differences)
}

/// Regular files (not symlinks) below `dir`, skipping `.svn`, as `/`-separated relative paths.
fn collect_regular_files(dir: &Path, prefix: &str, out: &mut BTreeSet<String>) -> Result<(), ReleaseError> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Ok(());
    };
    for entry in entries {
        let entry = entry.map_err(io_error(dir))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == ".svn" {
            continue;
        }
        let relative = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        let file_type = entry.file_type().map_err(io_error(&entry.path()))?;
        if file_type.is_dir() {
            collect_regular_files(&entry.path(), &relative, out)?;
        } else if file_type.is_file() {
            out.insert(relative);
        }
    }
    Ok(())
}

/// Replaces `destination` with `source`, scheduling it for addition when it is new.
pub async fn replace_file(destination: &Path, source: &Path, svn: &dyn SvnServicing) -> Result<(), ReleaseError> {
    let existed = file_exists(destination);
    if existed {
        if hashing::sha256_file(destination).ok() == hashing::sha256_file(source).ok() {
            return Ok(());
        }
        remove_item(destination).map_err(io_error(destination))?;
    }
    fs::copy(source, destination).map_err(io_error(source))?;
    if !existed {
        svn.add(&[destination.to_path_buf()]).await?;
    }
    Ok(())
}

/// Filesystem mirroring with content comparison. Never follows symlinks and never
/// touches `.svn` administrative directories.
pub mod file_mirror {
    use super::*;

    /// Existing paths under `target` whose kind (file vs directory) differs in the candidate.
    /// Only the outermost path of each change is returned.
    pub fn type_changes(entries: &[PackageEntry], target: &Path) -> Vec<PathBuf> {
        let mut sorted: Vec<&PackageEntry> = entries.iter().collect();
        sorted.sort_by_key(|e| e.path.chars().count());
        let mut changes: Vec<(String, PathBuf)> = Vec::new();
        for entry in sorted {
            if entry.kind == EntryKind::Symlink {
                continue;
            }
            if changes
                .iter()
                .any(|(relative, _)| entry.path.starts_with(&format!("{relative}/")))
            {
                continue;
            }
            let url = target.join(&entry.path);
            let (exists, is_directory) = file_exists_is_dir(&url);
            if !exists {
                continue;
            }
            if is_directory != (entry.kind == EntryKind::Directory) {
                changes.push((entry.path.clone(), url));
            }
        }
        changes.into_iter().map(|(_, url)| url).collect()
    }

    pub fn mirror(source: &Path, entries: &[PackageEntry], target: &Path) -> Result<(), ReleaseError> {
        fs::create_dir_all(target).map_err(io_error(target))?;
        let wanted_files: BTreeMap<&str, &PackageEntry> = entries
            .iter()
            .filter(|e| e.kind == EntryKind::File)
            .map(|e| (e.path.as_str(), e))
            .collect();
        let wanted_dirs: BTreeSet<&str> = entries
            .iter()
            .filter(|e| e.kind == EntryKind::Directory)
            .map(|e| e.path.as_str())
            .collect();

        // Remove what should not be there (deepest first).
        let mut existing: Vec<(String, bool)> = Vec::new();
        collect_existing(&standardized(target), "", &mut existing)?;
        existing.sort_by_key(|(path, _)| std::cmp::Reverse(path.chars().count()));
        for (path, is_dir) in existing {
            let keep = if is_dir {
                wanted_dirs.contains(path.as_str())
            } else {
                wanted_files.contains_key(path.as_str())
            };
            if !keep {
                let url = target.join(&path);
                remove_item(&url).map_err(io_error(&url))?;
            }
        }

        for dir in &wanted_dirs {
            let url = path_safety::resolve(&path_safety::validate_relative_path(dir)?, target)?;
            fs::create_dir_all(&url).map_err(io_error(&url))?;
        }
        for (path, entry) in &wanted_files {
            let destination = path_safety::resolve(&path_safety::validate_relative_path(path)?, target)?;
            let source_file = source.join(path);
            if file_exists(&destination)
                && entry
                    .sha256
                    .as_ref()
                    .is_some_and(|sha| hashing::sha256_file(&destination).ok().as_ref() == Some(sha))
            {
                continue;
            }
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent).map_err(io_error(parent))?;
            }
            if fs::symlink_metadata(&destination).is_ok() {
                remove_item(&destination).map_err(io_error(&destination))?;
            }
            fs::copy(&source_file, &destination).map_err(io_error(&source_file))?;
        }
        Ok(())
    }

    /// Everything below `dir` except `.svn` (and its contents): relative path and whether it
    /// is a real directory (a symlink to a directory counts as a file).
    fn collect_existing(dir: &Path, prefix: &str, out: &mut Vec<(String, bool)>) -> Result<(), ReleaseError> {
        let entries = fs::read_dir(dir).map_err(io_error(dir))?;
        for entry in entries {
            let entry = entry.map_err(io_error(dir))?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name == ".svn" {
                continue;
            }
            let relative = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            let is_dir = entry.file_type().map_err(io_error(&entry.path()))?.is_dir();
            out.push((relative.clone(), is_dir));
            if is_dir {
                collect_existing(&entry.path(), &relative, out)?;
            }
        }
        Ok(())
    }
}
