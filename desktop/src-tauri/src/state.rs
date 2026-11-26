use rd_store::{AppPaths, Store};
use rd_svn::SvnService;
use std::sync::Arc;

/// Shared services, opened once at launch. The data folder is the Swift app's
/// (`RELEASEDOCK_DATA_DIR` overrides it), so both apps see the same products and history.
pub struct AppState {
    pub paths: Option<AppPaths>,
    pub store: Result<Arc<Store>, String>,
    pub svn: Arc<SvnService>,
}

impl AppState {
    pub fn open() -> Self {
        let paths = AppPaths::default_root().map(AppPaths::new);
        let store = match &paths {
            Ok(paths) => Store::open(paths).map(Arc::new).map_err(|e| e.to_string()),
            Err(e) => Err(e.to_string()),
        };
        Self { paths: paths.ok(), store, svn: Arc::new(SvnService::new()) }
    }
}
