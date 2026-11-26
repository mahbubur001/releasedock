//! ReleaseDock's Tauri app: window setup and the commands the web UI calls. All release
//! logic lives in the `rd-*` crates in `rust/`; this layer only opens the shared state and
//! turns results into JSON for the UI.

mod commands;
mod product_tabs;
mod release;
mod state;

use state::AppState;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let state = AppState::open();
            if let Some(pipeline) = state.pipeline.clone() {
                release::reconcile_on_launch(pipeline.clone());
                release::forward_events(app.handle().clone(), pipeline);
            }
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::products,
            commands::product_icon,
            commands::releases,
            commands::svn_status,
            commands::ui_test_release,
            commands::ui_test_view,
            commands::log_ui_error,
            release::release,
            release::source_kind,
            release::prepare_release,
            release::approve_and_publish,
            release::cancel_release,
            release::recover_release,
            release::store_credentials,
            product_tabs::activity,
            product_tabs::released_versions,
            product_tabs::working_copy,
            product_tabs::update_working_copy,
            product_tabs::revert_working_copy,
            product_tabs::resolve_conflicts,
            product_tabs::transcript,
        ])
        .run(tauri::generate_context!())
        .expect("error while running ReleaseDock");
}
