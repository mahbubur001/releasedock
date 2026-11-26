//! Port of `Services/DiagnosticsService.swift`.

use crate::app_paths::AppPaths;
use crate::store::{ProductRepository, ReleaseRepository, StoreError, raw_value};
use rd_model::{SwiftDate, iso8601};
use rd_support::redact::redact;
use serde::Serialize;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

/// One application log line. Swift reads its entries from `OSLogStore`; the Rust app
/// supplies them through a [`LogSource`].
#[derive(Clone, Debug, PartialEq)]
pub struct LogEntry {
    pub date: SwiftDate,
    pub category: String,
    pub message: String,
}

/// Recent ReleaseDock log entries for the diagnostic bundle.
pub trait LogSource: Send + Sync {
    /// Entries written at or after `since`, oldest first.
    fn entries(&self, since: SwiftDate) -> Vec<LogEntry>;
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum DiagnosticsError {
    #[error(transparent)]
    Store(#[from] StoreError),
    /// Writing the bundle failed (Swift rethrows the Foundation error).
    #[error("{0}")]
    Io(String),
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProductInfo {
    name: String,
    slug: String,
    #[serde(rename = "type")]
    product_type: String,
    #[serde(rename = "svnURL")]
    svn_url: String,
    mode: String,
    automation_enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    current_version: Option<String>,
    #[serde(rename = "hasSVNUsername")]
    has_svn_username: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ReleaseInfo {
    id: String,
    product: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
    state: String,
    mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    committed_revision: Option<i64>,
    errors: Vec<String>,
    transitions: Vec<String>,
    blocking: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Bundle {
    /// `.iso8601` date strategy.
    generated_at: String,
    app: String,
    os: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    svn: Option<String>,
    products: Vec<ProductInfo>,
    releases: Vec<ReleaseInfo>,
    log: Vec<String>,
}

/// Builds a diagnostic bundle (JSON) for support: environment, products (without
/// credentials), recent releases with their state history, and recent ReleaseDock
/// log entries. Everything passes through `redact`.
pub struct DiagnosticsService {
    products: Arc<dyn ProductRepository>,
    releases: Arc<dyn ReleaseRepository>,
    paths: AppPaths,
    log: Option<Arc<dyn LogSource>>,
}

impl DiagnosticsService {
    pub fn new(products: Arc<dyn ProductRepository>, releases: Arc<dyn ReleaseRepository>, paths: AppPaths) -> Self {
        Self {
            products,
            releases,
            paths,
            log: None,
        }
    }

    /// Adds the source of the bundle's `log` lines (empty without one).
    pub fn with_log_source(mut self, log: Arc<dyn LogSource>) -> Self {
        self.log = Some(log);
        self
    }

    /// Writes the bundle into `Logs/` and returns its path.
    pub fn export(
        &self,
        app_version: &str,
        svn_version: Option<&str>,
        now: SwiftDate,
    ) -> Result<PathBuf, DiagnosticsError> {
        let products = self
            .products
            .all_products()?
            .into_iter()
            .map(|p| ProductInfo {
                name: p.name,
                slug: p.slug,
                product_type: raw_value(&p.product_type),
                svn_url: redact(&p.svn_repository_url),
                mode: raw_value(&p.release_mode),
                automation_enabled: p.automation_enabled,
                current_version: p.current_version,
                has_svn_username: p.svn_username.is_some_and(|u| !u.is_empty()),
            })
            .collect();
        let releases = self
            .releases
            .releases(None)?
            .into_iter()
            .take(50)
            .map(|r| ReleaseInfo {
                id: r.id.to_string(),
                product: r.product_slug,
                version: r.version,
                state: raw_value(&r.state),
                mode: raw_value(&r.mode),
                committed_revision: r.committed_revision,
                errors: r.errors.iter().map(|e| redact(e)).collect(),
                transitions: r
                    .transitions
                    .iter()
                    .map(|t| {
                        format!(
                            "{} {} → {}{}",
                            iso8601::format(t.at),
                            t.from.map_or_else(|| "-".to_owned(), |s| raw_value(&s)),
                            raw_value(&t.to),
                            t.note
                                .as_deref()
                                .map(|n| format!(": {}", redact(n)))
                                .unwrap_or_default()
                        )
                    })
                    .collect(),
                blocking: r
                    .validation
                    .as_ref()
                    .map(|v| {
                        v.blocking_results()
                            .iter()
                            .map(|b| format!("{}: {}", b.name, b.message))
                            .collect()
                    })
                    .unwrap_or_default(),
            })
            .collect();
        let bundle = Bundle {
            generated_at: iso8601::format(now),
            app: app_version.to_owned(),
            os: os_description(),
            svn: svn_version.map(str::to_owned),
            products,
            releases,
            log: self.recent_log_entries(now.adding(-24.0 * 3600.0)),
        };
        let value = serde_json::to_value(&bundle).map_err(|e| DiagnosticsError::Io(e.to_string()))?;
        let json = rd_model::swift_json::to_pretty(&value);

        let logs = self.paths.logs();
        fs::create_dir_all(&logs).map_err(io_error)?;
        let stamp = iso8601::format(now).replace(':', "-");
        let path = logs.join(format!("ReleaseDock-diagnostics-{stamp}.json"));
        write_atomically(&path, json.as_bytes())?;
        Ok(path)
    }

    /// The log source's entries, last 2000, each `<iso8601> [<category>] <redacted message>`.
    fn recent_log_entries(&self, since: SwiftDate) -> Vec<String> {
        let Some(log) = &self.log else { return Vec::new() };
        let entries = log.entries(since);
        let skip = entries.len().saturating_sub(2000);
        entries
            .into_iter()
            .skip(skip)
            .map(|e| format!("{} [{}] {}", iso8601::format(e.date), e.category, redact(&e.message)))
            .collect()
    }
}

/// Stands in for `ProcessInfo.operatingSystemVersionString`, which has no portable equivalent.
fn os_description() -> String {
    format!("{} {}", std::env::consts::OS, std::env::consts::ARCH)
}

fn io_error(error: std::io::Error) -> DiagnosticsError {
    DiagnosticsError::Io(error.to_string())
}

/// `Data.write(to:options: [.atomic])`: write a sibling temporary file, then rename it.
fn write_atomically(path: &std::path::Path, data: &[u8]) -> Result<(), DiagnosticsError> {
    let temporary = path.with_extension("json.tmp");
    let mut file = fs::File::create(&temporary).map_err(io_error)?;
    file.write_all(data).map_err(io_error)?;
    file.sync_all().map_err(io_error)?;
    drop(file);
    fs::rename(&temporary, path).map_err(io_error)
}
