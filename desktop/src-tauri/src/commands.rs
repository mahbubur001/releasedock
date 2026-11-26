//! Commands the UI calls with `invoke`. Errors are returned as plain strings the UI shows.

use crate::state::AppState;
use rd_model::{Id, Product};
use rd_records::ReleaseRecord;
use rd_store::{ProductRepository, ReleaseRepository};
use rd_svn::SvnServicing;
use serde::Serialize;
use tauri::State;

type CommandResult<T> = Result<T, String>;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    version: String,
    platform: &'static str,
    data_folder: Option<String>,
    store_error: Option<String>,
}

#[tauri::command]
pub fn app_info(app: tauri::AppHandle, state: State<'_, AppState>) -> AppInfo {
    AppInfo {
        version: app.package_info().version.to_string(),
        platform: std::env::consts::OS,
        data_folder: state.paths.as_ref().map(|p| p.root.display().to_string()),
        store_error: state.store.as_ref().err().cloned(),
    }
}

#[tauri::command]
pub fn products(state: State<'_, AppState>) -> CommandResult<Vec<Product>> {
    let store = state.store.as_ref().map_err(Clone::clone)?;
    let mut products = store.all_products().map_err(|e| e.to_string())?;
    products.sort_by_key(|p| p.name.to_lowercase());
    Ok(products)
}

/// One row of release history: enough for lists without sending whole inspections.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseRow {
    id: String,
    product_id: String,
    product_name: String,
    version: Option<String>,
    previous_version: Option<String>,
    state: String,
    kind: String,
    mode: String,
    committed_revision: Option<i64>,
    /// Milliseconds since 1970, for JavaScript `Date`.
    created_at: f64,
}

fn raw<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default()
}

#[tauri::command]
pub fn releases(state: State<'_, AppState>, product_id: Option<String>) -> CommandResult<Vec<ReleaseRow>> {
    let store = state.store.as_ref().map_err(Clone::clone)?;
    let filter = product_id.as_deref().map(|id| Id::parse(id).ok_or("Invalid product id")).transpose()?;
    let mut records: Vec<ReleaseRecord> = store.releases(filter).map_err(|e| e.to_string())?;
    records.sort_by(|a, b| b.created_at.0.total_cmp(&a.created_at.0));
    Ok(records
        .iter()
        .map(|r| ReleaseRow {
            id: r.id.to_string(),
            product_id: r.product_id.to_string(),
            product_name: r.product_name.clone(),
            version: r.version.clone(),
            previous_version: r.previous_version.clone(),
            state: raw(&r.state),
            kind: raw(&r.kind()),
            mode: raw(&r.mode),
            committed_revision: r.committed_revision,
            created_at: r.created_at.unix_seconds() * 1000.0,
        })
        .collect())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SvnStatus {
    version: Option<String>,
    executable: Option<String>,
    problem: Option<String>,
}

#[tauri::command]
pub async fn svn_status(state: State<'_, AppState>) -> CommandResult<SvnStatus> {
    Ok(match state.svn.detect_installation().await {
        Ok(installation) => SvnStatus {
            version: Some(installation.version),
            executable: Some(installation.executable.display().to_string()),
            problem: None,
        },
        Err(error) => SvnStatus { version: None, executable: None, problem: Some(error.to_string()) },
    })
}

/// The product's WordPress.org directory icon as a data URL, from its assets folder or the
/// working copy's SVN `assets/` folder; None when it has none.
#[tauri::command]
pub fn product_icon(state: State<'_, AppState>, product_id: String) -> CommandResult<Option<String>> {
    use base64::Engine;
    const NAMES: [(&str, &str); 7] = [
        ("icon-256x256.png", "image/png"),
        ("icon-128x128.png", "image/png"),
        ("icon-256x256.jpg", "image/jpeg"),
        ("icon-128x128.jpg", "image/jpeg"),
        ("icon-256x256.gif", "image/gif"),
        ("icon-128x128.gif", "image/gif"),
        ("icon.svg", "image/svg+xml"),
    ];
    let store = state.store.as_ref().map_err(Clone::clone)?;
    let id = Id::parse(&product_id).ok_or("Invalid product id")?;
    let Some(product) = store.product(id).map_err(|e| e.to_string())? else { return Ok(None) };
    let folders = product
        .assets_path
        .iter()
        .map(std::path::PathBuf::from)
        .chain(std::iter::once(product.working_copy_dir().join("assets")));
    for folder in folders {
        for (name, mime) in NAMES {
            if let Ok(bytes) = std::fs::read(folder.join(name)) {
                let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
                return Ok(Some(format!("data:{mime};base64,{encoded}")));
            }
        }
    }
    Ok(None)
}
