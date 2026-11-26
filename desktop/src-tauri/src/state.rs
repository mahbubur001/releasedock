use rd_integrations::{BuildService, CodeCheckConfiguration, CodeCheckConfigurationStore, CodeCheckService, GitService, GitHubService, WordPressOrgService};
use rd_keychain::NativeKeychain;
use rd_pipeline::{PipelineDependencies, ReleasePipelineService, ReleaseStagingService};
use rd_store::{AppPaths, SettingsRepository, Store};
use rd_svn::{SvnService, SvnTranscript};
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;

/// The Swift app's preferences (`AppSettings`, key `app.settings`). Unknown keys are ignored
/// and missing ones take Swift's defaults, so both apps share one settings record.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    pub svn_executable_path: String,
    pub plugin_check_word_press_path: Option<String>,
    pub commit_timeout_minutes: Option<u64>,
}

pub const DEFAULT_COMMIT_TIMEOUT_MINUTES: u64 = 30;

/// Shared services, opened once at launch. The data folder is the Swift app's
/// (`RELEASEDOCK_DATA_DIR` overrides it), so both apps see the same products and history.
pub struct AppState {
    pub paths: Option<AppPaths>,
    pub store: Result<Arc<Store>, String>,
    pub svn: Arc<SvnService>,
    pub transcript: Option<Arc<SvnTranscript>>,
    pub keychain: Arc<NativeKeychain>,
    pub pipeline: Option<Arc<ReleasePipelineService>>,
}

impl AppState {
    pub fn open() -> Self {
        let paths = AppPaths::default_root().map(AppPaths::new).ok();
        let store = match &paths {
            Some(paths) => paths
                .create_all()
                .map_err(|e| e.to_string())
                .and_then(|_| Store::open(paths).map(Arc::new).map_err(|e| e.to_string())),
            None => Err("ReleaseDock could not find its data folder.".into()),
        };
        let settings: AppSettings = store
            .as_ref()
            .ok()
            .and_then(|s| s.setting::<AppSettings>("app.settings").ok().flatten())
            .unwrap_or_default();
        let minutes = settings.commit_timeout_minutes.unwrap_or(DEFAULT_COMMIT_TIMEOUT_MINUTES);
        let transcript = paths.as_ref().map(|p| Arc::new(SvnTranscript::new(p.transcripts())));
        let mut svn = SvnService::new()
            .with_commit_timeout(Duration::from_secs(minutes * 60))
            .with_commit_http_timeout_seconds(minutes * 60);
        if !settings.svn_executable_path.is_empty() {
            svn = svn.with_executable(&settings.svn_executable_path);
        }
        if let Some(transcript) = &transcript {
            svn = svn.with_transcript(transcript.clone());
        }
        let svn = Arc::new(svn);
        let keychain = Arc::new(NativeKeychain);
        let pipeline = match (&store, &paths) {
            (Ok(store), Some(paths)) => {
                let code_checks = CodeCheckService::new().with_configuration(CodeCheckConfigurationStore::new(
                    CodeCheckConfiguration::new(settings.plugin_check_word_press_path.clone()),
                ));
                let dependencies = PipelineDependencies::new(
                    store.clone(),
                    store.clone(),
                    svn.clone(),
                    Arc::new(GitService::new()),
                    keychain.clone(),
                    Arc::new(ReleaseStagingService::new(paths.clone())),
                )
                .with_wordpress_org(WordPressOrgService::new().ok().map(|s| Arc::new(s) as _))
                .with_code_checker(Some(Arc::new(code_checks)))
                .with_builder(Some(Arc::new(BuildService::new())))
                .with_github_releaser(GitHubService::new().ok().map(|s| Arc::new(s) as _))
                .with_generator(format!("ReleaseDock {} (desktop)", env!("CARGO_PKG_VERSION")));
                Some(Arc::new(ReleasePipelineService::new(dependencies)))
            }
            _ => None,
        };
        Self { paths, store, svn, transcript, keychain, pipeline }
    }
}
