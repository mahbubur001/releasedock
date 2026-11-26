//! The repository parts of the release strategies that `RepositorySetupService` needs:
//! `inspectRemote` and `prepareWorkingCopy` of `Release/PluginReleaseStrategy.swift` and
//! `Release/ThemeReleaseStrategy.swift`, and `WorkingCopyOperations.catIfExists` /
//! `localChanges` / `changes` from `Release/ReleaseStrategy.swift`.
//!
//! The rest of the strategies (validation, apply, verify) belongs to the release pipeline;
//! when that is ported it should call these functions instead of copying them.

use crate::url_text::trimming_trailing_slash;
use rd_inspection::FileClassifier;
use rd_model::{Product, ProductType};
use rd_records::{RepositorySnapshot, WorkingCopyChange};
use rd_svn::{SvnDepth, SvnError, SvnErrorKind, SvnItemStatus, SvnNodeKind, SvnServicing};
use rd_versioning::{VersionService, VersionServicing, readme};
use std::path::Path;

/// Errors of the working-copy preparation: svn failures and the local folder.
#[derive(Debug, thiserror::Error)]
pub enum LayoutError {
    #[error(transparent)]
    Svn(#[from] SvnError),
    #[error("{0}")]
    Io(String),
}

/// `ThemeReleaseStrategy.isVersionDirectory`: theme releases are `/<version>/` directories.
pub fn is_theme_version_directory(name: &str) -> bool {
    name.chars().next().is_some_and(char::is_numeric)
}

/// `readme.txt` (preferred) or `readme.md`, matched case-insensitively like WordPress.org.
pub fn plugin_readme_name(files: &[String]) -> Option<&str> {
    ["readme.txt", "readme.md"]
        .iter()
        .find_map(|wanted| files.iter().find(|name| name.to_lowercase() == *wanted))
        .map(String::as_str)
}

/// `svn cat` that returns None only when the file is positively absent; other failures fail.
pub async fn cat_if_exists(url: &str, svn: &dyn SvnServicing) -> Result<Option<Vec<u8>>, SvnError> {
    match svn.cat(url).await {
        Ok(data) => Ok(Some(data)),
        Err(error) if error.kind() == Some(SvnErrorKind::NotFound) => Ok(None),
        Err(error) => Err(error),
    }
}

/// Reads remote state: structure, released version, existing versions, and whether
/// `candidate_version`'s release path exists.
pub async fn inspect_remote(
    product: &Product,
    candidate_version: Option<&str>,
    svn: &dyn SvnServicing,
) -> Result<RepositorySnapshot, SvnError> {
    match product.product_type {
        ProductType::Plugin => inspect_plugin_remote(product, candidate_version, svn).await,
        ProductType::Theme => inspect_theme_remote(product, candidate_version, svn).await,
    }
}

async fn inspect_plugin_remote(
    product: &Product,
    candidate_version: Option<&str>,
    svn: &dyn SvnServicing,
) -> Result<RepositorySnapshot, SvnError> {
    let mut snapshot = RepositorySnapshot::default();
    let base = trimming_trailing_slash(&product.svn_repository_url);
    // Independent requests run concurrently: each WordPress.org round trip takes seconds.
    let (info, top) = tokio::join!(svn.info(base), svn.list(base, false));
    let info = match info {
        Ok(info) => info,
        Err(error) if error.kind() == Some(SvnErrorKind::NotFound) => {
            snapshot.repository_exists = Some(false);
            return Ok(snapshot);
        }
        Err(error) => return Err(error),
    };
    snapshot.repository_exists = Some(true);
    snapshot.remote_revision = Some(info.last_changed_revision.unwrap_or(info.revision));

    let names: Vec<String> = top?
        .into_iter()
        .filter(|e| e.kind == SvnNodeKind::Dir)
        .map(|e| e.name)
        .collect();
    for required in ["trunk", "tags"] {
        if !names.iter().any(|n| n == required) {
            snapshot.structure_problems.push(format!(
                "Repository has no /{required} directory; this does not look like a WordPress.org plugin repository"
            ));
        }
    }
    if !snapshot.structure_problems.is_empty() {
        return Ok(snapshot);
    }

    let tags_url = format!("{base}/tags");
    let trunk_url = format!("{base}/trunk");
    let (tags, trunk) = tokio::join!(svn.list(&tags_url, false), svn.list(&trunk_url, false));
    let tags: Vec<String> = tags?
        .into_iter()
        .filter(|e| e.kind == SvnNodeKind::Dir)
        .map(|e| e.name)
        .collect();
    let trunk_files: Vec<String> = trunk?
        .into_iter()
        .filter(|e| e.kind == SvnNodeKind::File)
        .map(|e| e.name)
        .collect();
    snapshot.existing_versions = tags.clone();

    // Released version = trunk readme Stable tag when that tag exists; otherwise the
    // highest tag (what the importer falls back to when trunk is empty). WordPress.org
    // matches the readme name case-insensitively, preferring readme.txt.
    let mut stable: Option<String> = None;
    if let Some(readme_name) = plugin_readme_name(&trunk_files) {
        if let Some(data) = cat_if_exists(&format!("{base}/trunk/{readme_name}"), svn).await? {
            let parsed = readme::parse(&String::from_utf8_lossy(&data));
            stable = parsed.stable_tag();
            snapshot.readme_title = parsed.name;
        }
    }
    let stable = stable.filter(|s| !s.is_empty());
    match &stable {
        Some(stable) if stable != "trunk" && tags.contains(stable) => {
            snapshot.current_version = Some(stable.clone());
            snapshot.current_version_source = Some("trunk readme Stable tag".into());
        }
        _ => {
            if let Some(highest) = VersionService::new().highest(&tags) {
                snapshot.current_version = Some(highest);
                snapshot.current_version_source = Some("highest tag".into());
                if let Some(stable) = &stable {
                    snapshot.notes.push(format!(
                        "Current Stable tag \"{stable}\" does not point to an existing tag; WordPress.org is serving trunk."
                    ));
                }
            }
        }
    }
    if let Some(candidate) = candidate_version {
        snapshot.release_path_exists = Some(tags.iter().any(|t| t == candidate));
    }
    Ok(snapshot)
}

async fn inspect_theme_remote(
    product: &Product,
    candidate_version: Option<&str>,
    svn: &dyn SvnServicing,
) -> Result<RepositorySnapshot, SvnError> {
    let mut snapshot = RepositorySnapshot::default();
    let base = trimming_trailing_slash(&product.svn_repository_url);
    let (info, list) = tokio::join!(svn.info(base), svn.list(base, false));
    let info = match info {
        Ok(info) => info,
        Err(error) if error.kind() == Some(SvnErrorKind::NotFound) => {
            snapshot.repository_exists = Some(false);
            snapshot.structure_problems.push("The theme has no SVN directory. New themes must be submitted at https://wordpress.org/themes/upload/ and pass review first.".into());
            return Ok(snapshot);
        }
        Err(error) => return Err(error),
    };
    snapshot.repository_exists = Some(true);
    snapshot.remote_revision = Some(info.last_changed_revision.unwrap_or(info.revision));
    let entries = list?;
    let versions: Vec<String> = entries
        .iter()
        .filter(|e| e.kind == SvnNodeKind::Dir && is_theme_version_directory(&e.name))
        .map(|e| e.name.clone())
        .collect();
    snapshot.existing_versions = versions.clone();
    if entries.iter().any(|e| e.name == "trunk" || e.name == "tags") {
        snapshot.notes.push(
            "The repository contains trunk/ or tags/; WordPress.org themes only import /<version>/ directories.".into(),
        );
    }
    if let Some(highest) = VersionService::new().highest(&versions) {
        snapshot.current_version = Some(highest);
        snapshot.current_version_source = Some("highest version directory".into());
    } else {
        snapshot.structure_problems.push("No released version directory exists. The first version of a theme must go through the WordPress.org upload and review process.".into());
    }
    if let Some(candidate) = candidate_version {
        snapshot.release_path_exists = Some(versions.iter().any(|v| v == candidate));
    }
    Ok(snapshot)
}

/// Checks out (sparsely) or updates the working copy so it is ready for staging.
/// `current_version` is the latest released version (themes copy from it).
pub async fn prepare_working_copy(
    product: &Product,
    current_version: Option<&str>,
    svn: &dyn SvnServicing,
) -> Result<(), LayoutError> {
    let wc = product.working_copy_dir();
    let fresh = !wc.join(".svn").exists();
    if fresh {
        std::fs::create_dir_all(&wc).map_err(|e| LayoutError::Io(format!("{}: {e}", wc.display())))?;
    }
    match product.product_type {
        ProductType::Plugin => {
            if fresh {
                // Sparse checkout: never download every tag.
                svn.checkout(&product.svn_repository_url, &wc, SvnDepth::Immediates)
                    .await?;
                svn.update(&wc.join("trunk"), Some(SvnDepth::Infinity)).await?;
                svn.update(&wc.join("tags"), Some(SvnDepth::Immediates)).await?;
            } else {
                svn.update(&wc, None).await?;
            }
            if product.assets_path.is_some() && wc.join("assets").exists() {
                svn.update(&wc.join("assets"), Some(SvnDepth::Infinity)).await?;
            }
        }
        ProductType::Theme => {
            if fresh {
                // Version directories are checked out empty; only the previous one is filled.
                svn.checkout(&product.svn_repository_url, &wc, SvnDepth::Immediates)
                    .await?;
            } else {
                svn.update(&wc, None).await?;
            }
            if let Some(current_version) = current_version {
                svn.update(&wc.join(current_version), Some(SvnDepth::Infinity)).await?;
            }
        }
    }
    Ok(())
}

/// `status: path` lines of [`changes`] (`modified: trunk/readme.txt`).
pub async fn local_changes(path: &Path, svn: &dyn SvnServicing) -> Result<Vec<String>, SvnError> {
    Ok(changes(path, svn)
        .await?
        .into_iter()
        .map(|change| format!("{}: {}", status_raw_value(change.status), change.path))
        .collect())
}

/// Local changes that make the working copy dirty, with paths relative to `path`.
pub async fn changes(path: &Path, svn: &dyn SvnServicing) -> Result<Vec<WorkingCopyChange>, SvnError> {
    let root = path.to_string_lossy().trim_end_matches(['/', '\\']).to_owned();
    Ok(svn
        .status(path, false)
        .await?
        .into_iter()
        .filter(|entry| {
            if entry.is_local_change() || entry.item == SvnItemStatus::Unversioned {
                return true;
            }
            // Ignored files would be overwritten or deleted by staging; only OS junk is exempt.
            if entry.item != SvnItemStatus::Ignored {
                return false;
            }
            let name = entry.path.rsplit(['/', '\\']).next().unwrap_or(&entry.path);
            !FileClassifier::is_junk_name(name)
        })
        .map(|entry| {
            let relative = entry
                .path
                .strip_prefix(&root)
                .and_then(|rest| rest.strip_prefix('/').or_else(|| rest.strip_prefix('\\')))
                .map_or_else(|| entry.path.clone(), str::to_owned);
            WorkingCopyChange {
                path: relative,
                status: entry.item,
                is_conflict: entry.is_conflict(),
                is_tree_conflict: entry.is_tree_conflict,
            }
        })
        .collect())
}

/// The Swift raw value (`modified`, `unversioned`…).
fn status_raw_value(status: SvnItemStatus) -> String {
    serde_json::to_value(status)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default()
}
