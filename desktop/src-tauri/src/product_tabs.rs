//! Commands behind a product's Activity, Releases and Files tabs, its transcript, and the
//! working-copy actions (Update, Revert, resolve conflicts).

use crate::release::CommandError;
use crate::state::AppState;
use rd_model::{Id, Product};
use rd_records::{ConflictResolution, WorkingCopyState};
use rd_store::ProductRepository;
use rd_svn::{SvnServicing, SvnTranscribing, TranscriptEntry};
use serde::Serialize;
use std::sync::Arc;
use tauri::State;

type Result<T> = std::result::Result<T, CommandError>;

fn product(state: &AppState, product_id: &str) -> Result<Product> {
    let store = state.store.as_ref().map_err(|e| CommandError::from(e.clone()))?;
    let id = Id::parse(product_id).ok_or("Invalid product id")?;
    store.product(id).map_err(|e| e.to_string())?.ok_or_else(|| "The product no longer exists.".into())
}

fn millis(date: Option<&str>) -> Option<f64> {
    date.and_then(rd_model::iso8601::parse).map(|d| d.unix_seconds() * 1000.0)
}

/// The ReleaseDock commit marker, as written by the pipeline.
const MARKER: &str = "ReleaseDock-Release-ID:";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangedPath {
    action: String,
    /// Relative to the product root, e.g. `trunk/readme.txt`.
    path: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityEntry {
    revision: i64,
    author: Option<String>,
    date: Option<f64>,
    /// The summary, without ReleaseDock's release-ID line.
    message: String,
    paths: Vec<ChangedPath>,
    /// `1.2.0` when the commit created `tags/1.2.0`.
    tag: Option<String>,
    by_release_dock: bool,
}

/// Recent commits to this product only (never the whole shared repository).
#[tauri::command]
pub async fn activity(state: State<'_, AppState>, product_id: String, limit: Option<usize>) -> Result<Vec<ActivityEntry>> {
    let product = product(&state, &product_id)?;
    let root_path = url_path(&product.svn_repository_url);
    let entries = state.svn.log(&product.svn_repository_url, limit.unwrap_or(50)).await.map_err(|e| e.to_string())?;
    Ok(entries
        .into_iter()
        .map(|e| {
            let paths: Vec<ChangedPath> = e
                .changed_paths
                .iter()
                .zip(e.changed_path_actions.iter().chain(std::iter::repeat(&"M".to_owned())))
                .map(|(path, action)| ChangedPath {
                    action: action.clone(),
                    path: relative(path, &root_path),
                })
                .collect();
            let tag = paths.iter().find_map(|p| {
                (p.action == "A").then(|| p.path.strip_prefix("tags/")).flatten().filter(|t| !t.contains('/')).map(str::to_owned)
            });
            let by_release_dock = e.message.contains(MARKER);
            let message = e.message.lines().filter(|l| !l.starts_with(MARKER)).collect::<Vec<_>>().join("\n").trim().to_owned();
            ActivityEntry { revision: e.revision, author: e.author, date: millis(e.date.as_deref()), message, paths, tag, by_release_dock }
        })
        .collect())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TagRow {
    version: String,
    revision: Option<i64>,
    date: Option<f64>,
    author: Option<String>,
    by_release_dock: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleasesTab {
    tags: Vec<TagRow>,
    stable_tag: Option<String>,
}

/// Every `tags/<version>`, newest version first, with the trunk readme's Stable tag.
#[tauri::command]
pub async fn released_versions(state: State<'_, AppState>, product_id: String) -> Result<ReleasesTab> {
    let product = product(&state, &product_id)?;
    let base = product.svn_repository_url.trim_end_matches('/').to_owned();
    let svn: Arc<rd_svn::SvnService> = state.svn.clone();
    let (tags_url, readme_url) = (format!("{base}/tags"), format!("{base}/trunk/readme.txt"));
    let (list, log, readme) = tokio::join!(svn.list(&tags_url, false), svn.log(&tags_url, 200), svn.cat(&readme_url));
    let list = list.map_err(|e| e.to_string())?;
    let log = log.unwrap_or_default();
    let mut tags: Vec<TagRow> = list
        .into_iter()
        .filter(|e| e.kind == rd_svn::SvnNodeKind::Dir)
        .map(|e| {
            let version = e.name.trim_end_matches('/').to_owned();
            let commit = e.commit_revision.and_then(|r| log.iter().find(|l| l.revision == r));
            TagRow {
                revision: e.commit_revision,
                date: commit.and_then(|c| millis(c.date.as_deref())),
                author: commit.and_then(|c| c.author.clone()),
                by_release_dock: commit.is_some_and(|c| c.message.contains(MARKER)),
                version,
            }
        })
        .collect();
    tags.sort_by(|a, b| rd_versioning::php_version::compare(&b.version, &a.version).cmp(&0));
    let stable_tag = readme.ok().and_then(|bytes| rd_versioning::readme::parse(&String::from_utf8_lossy(&bytes)).stable_tag());
    Ok(ReleasesTab { tags, stable_tag })
}

#[tauri::command]
pub async fn working_copy(state: State<'_, AppState>, product_id: String) -> Result<WorkingCopyState> {
    let id = Id::parse(&product_id).ok_or("Invalid product id")?;
    let pipeline = state.pipeline.clone().ok_or("Releases are unavailable.")?;
    Ok(pipeline.working_copy_state(id).await?)
}

#[tauri::command]
pub async fn update_working_copy(state: State<'_, AppState>, product_id: String) -> Result<WorkingCopyState> {
    let id = Id::parse(&product_id).ok_or("Invalid product id")?;
    let pipeline = state.pipeline.clone().ok_or("Releases are unavailable.")?;
    Ok(pipeline.update_working_copy(id).await?)
}

/// Backs everything up, then reverts; returns the backup's path.
#[tauri::command]
pub async fn revert_working_copy(state: State<'_, AppState>, product_id: String) -> Result<Option<String>> {
    let id = Id::parse(&product_id).ok_or("Invalid product id")?;
    let pipeline = state.pipeline.clone().ok_or("Releases are unavailable.")?;
    Ok(pipeline.revert_working_copy(id).await?.map(|p| p.display().to_string()))
}

#[tauri::command]
pub async fn resolve_conflicts(
    state: State<'_, AppState>,
    product_id: String,
    paths: Vec<String>,
    keeping: ConflictResolution,
) -> Result<WorkingCopyState> {
    let id = Id::parse(&product_id).ok_or("Invalid product id")?;
    let pipeline = state.pipeline.clone().ok_or("Releases are unavailable.")?;
    Ok(pipeline.resolve_conflicts(id, &paths, keeping).await?.0)
}

/// svn commands ReleaseDock ran for this product (or one release), oldest first.
#[tauri::command]
pub fn transcript(state: State<'_, AppState>, product_id: String, release_id: Option<String>) -> Result<Vec<TranscriptEntry>> {
    let id = Id::parse(&product_id).ok_or("Invalid product id")?;
    let release = release_id.as_deref().map(|r| Id::parse(r).ok_or("Invalid release id")).transpose()?;
    Ok(state.transcript.as_ref().map(|t| t.entries(id, release, 400)).unwrap_or_default())
}

/// `https://plugins.svn.wordpress.org/radius-booking` → `/radius-booking`.
fn url_path(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let path = rest.find('/').map_or("", |i| &rest[i..]);
    path.trim_end_matches('/').to_owned()
}

fn relative(path: &str, root: &str) -> String {
    path.strip_prefix(root).unwrap_or(path).trim_start_matches('/').to_owned()
}
