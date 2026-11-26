//! ReleaseDock's Tauri app: window setup and the commands the web UI calls. All release
//! logic lives in the `rd-*` crates in `rust/`; this layer only opens the shared state and
//! turns results into JSON for the UI.

mod commands;
mod state;

use state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::open())
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::products,
            commands::product_icon,
            commands::releases,
            commands::svn_status,
        ])
        .run(tauri::generate_context!())
        .expect("error while running ReleaseDock");
}
