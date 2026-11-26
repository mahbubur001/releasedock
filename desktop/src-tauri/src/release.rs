//! New Release commands: create and prepare a release, approve and publish it, cancel it,
//! and store SVN credentials. Progress arrives as `pipeline-event` events.

use crate::state::AppState;
use rd_keychain::{Keychain, KeychainItem};
use rd_model::{Id, ReleaseMode};
use rd_pipeline::{ReleaseError, ReleasePipelineService};
use rd_records::{ApprovalKind, ReleaseKind, ReleaseRecord, ReleaseSource};
use rd_store::ProductRepository;
use serde::Serialize;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};

/// An error the UI can act on: `credentials` asks for the SVN password.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandError {
    message: String,
    needs_credentials: bool,
}

impl From<ReleaseError> for CommandError {
    fn from(error: ReleaseError) -> Self {
        let text = error.to_string();
        let needs_credentials = matches!(error, ReleaseError::CredentialsRequired)
            || text.contains("SVN credentials are required")
            || text.contains("SVN authentication failed");
        Self { message: text, needs_credentials }
    }
}

impl From<String> for CommandError {
    fn from(message: String) -> Self {
        Self { message, needs_credentials: false }
    }
}

impl From<&str> for CommandError {
    fn from(message: &str) -> Self {
        message.to_owned().into()
    }
}

type Result<T> = std::result::Result<T, CommandError>;

fn pipeline(state: &AppState) -> Result<Arc<ReleasePipelineService>> {
    state.pipeline.clone().ok_or_else(|| "ReleaseDock could not open its data, so releases are unavailable.".into())
}

fn id(text: &str) -> Result<Id> {
    Id::parse(text).ok_or_else(|| "Invalid id".into())
}

/// Forwards pipeline progress to the window as `pipeline-event`.
pub fn forward_events(app: AppHandle, pipeline: Arc<ReleasePipelineService>) {
    let mut events = pipeline.events();
    tauri::async_runtime::spawn(async move {
        loop {
            match events.recv().await {
                Ok(event) => {
                    let _ = app.emit(
                        "pipeline-event",
                        serde_json::json!({
                            "releaseId": event.release_id.to_string(),
                            "state": event.state,
                            "message": event.message,
                            "at": event.at.unix_seconds() * 1000.0,
                        }),
                    );
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

/// Releases interrupted by a quit or crash become recoverable before anything else runs.
pub fn reconcile_on_launch(pipeline: Arc<ReleasePipelineService>) {
    tauri::async_runtime::spawn(async move {
        if let Err(error) = pipeline.reconcile_interrupted_releases().await {
            log_error(&format!("Could not check interrupted releases: {error}"));
        }
    });
}

fn log_error(message: &str) {
    eprintln!("ReleaseDock: {message}");
}

#[tauri::command]
pub fn release(state: State<'_, AppState>, release_id: String) -> Result<Option<ReleaseRecord>> {
    let store = state.store.as_ref().map_err(|e| CommandError::from(e.clone()))?;
    rd_store::ReleaseRepository::release(store.as_ref(), id(&release_id)?).map_err(|e| e.to_string().into())
}

/// Whether `path` is a ZIP file, a folder, or neither.
#[tauri::command]
pub fn source_kind(path: String) -> Option<&'static str> {
    let path = std::path::Path::new(&path);
    if path.is_dir() {
        Some("folder")
    } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("zip")) && path.is_file() {
        Some("zip")
    } else {
        None
    }
}

/// Detect → Inspect → Validate → Stage → Preview (a dry run completes here).
#[tauri::command]
pub async fn prepare_release(
    state: State<'_, AppState>,
    product_id: String,
    source_path: String,
    mode: ReleaseMode,
    kind: ReleaseKind,
    commit_template: Option<String>,
) -> Result<ReleaseRecord> {
    let pipeline = pipeline(&state)?;
    let source = match source_kind(source_path.clone()) {
        Some("zip") => ReleaseSource::zip(source_path),
        Some(_) => ReleaseSource::folder(source_path),
        None => return Err("Choose a ZIP file or a folder.".into()),
    };
    let template = commit_template.as_deref().map(str::trim).filter(|t| !t.is_empty());
    let created = pipeline.create_release(id(&product_id)?, source, Some(mode), kind, template)?;
    Ok(pipeline.prepare(created.id).await?)
}

#[tauri::command]
pub async fn approve_and_publish(state: State<'_, AppState>, release_id: String) -> Result<ReleaseRecord> {
    let pipeline = pipeline(&state)?;
    let id = id(&release_id)?;
    let current = rd_store::ReleaseRepository::release(state.store.as_ref().map_err(|e| CommandError::from(e.clone()))?.as_ref(), id)
        .map_err(|e| e.to_string())?;
    if current.is_some_and(|r| r.state != rd_records::ReleaseState::Approved) {
        pipeline.approve(id, ApprovalKind::User).await?;
    }
    Ok(pipeline.publish(id).await?)
}

#[tauri::command]
pub async fn cancel_release(state: State<'_, AppState>, release_id: String) -> Result<ReleaseRecord> {
    Ok(pipeline(&state)?.cancel(id(&release_id)?).await?)
}

#[tauri::command]
pub async fn recover_release(state: State<'_, AppState>, release_id: String) -> Result<ReleaseRecord> {
    Ok(pipeline(&state)?.recover(id(&release_id)?).await?)
}

/// Saves the SVN password in the OS credential store and the username on the product.
#[tauri::command]
pub fn store_credentials(state: State<'_, AppState>, product_id: String, username: String, password: String) -> Result<()> {
    let store = state.store.as_ref().map_err(|e| CommandError::from(e.clone()))?;
    let mut product = store.product(id(&product_id)?).map_err(|e| e.to_string())?.ok_or("The product no longer exists.")?;
    let host = url_host(&product.svn_repository_url).ok_or("The product's SVN URL has no host.")?;
    if let Some(previous) = product.svn_username.as_deref().filter(|p| *p != username) {
        let _ = state.keychain.delete_password(&KeychainItem::svn(&host, previous));
    }
    state.keychain.set_password(&password, &KeychainItem::svn(&host, &username)).map_err(|e| e.to_string())?;
    product.svn_username = Some(username);
    store.save_product(&product).map_err(|e| e.to_string())?;
    Ok(())
}

fn url_host(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let host = rest.split(['/', '?', '#']).next()?;
    let host = host.rsplit_once('@').map_or(host, |(_, h)| h);
    Some(host.split(':').next()?.to_owned()).filter(|h| !h.is_empty())
}
