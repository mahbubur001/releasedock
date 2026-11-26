//! Port of `CodeChecks/CodeCheckService.swift`. The stored report types
//! (`CodeCheckTool`, `CodeCheckFinding`, `CodeCheckReport`) live in `rd-records`.

use crate::discovery::{PHP_CANDIDATES, WP_CANDIDATES, find_executable};
use async_trait::async_trait;
use rd_model::{Product, ProductType, ValidationSeverity};
use rd_records::{CodeCheckFinding, CodeCheckReport, CodeCheckTool};
use rd_svn::{ProcessRequest, ProcessRunner, ProcessRunning};
use rd_versioning::php_version;
use regex::Regex;
use serde::Deserialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, RwLock};
use std::time::Duration;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CodeCheckConfiguration {
    /// Root of a local WordPress install with the Plugin Check plugin active. None or empty
    /// disables Plugin Check.
    pub plugin_check_word_press_path: Option<String>,
}

impl CodeCheckConfiguration {
    pub fn new(plugin_check_word_press_path: Option<String>) -> Self {
        Self {
            plugin_check_word_press_path,
        }
    }
}

/// Thread-safe holder so the app can change the configuration of a long-lived service.
/// Clones share the same value.
#[derive(Clone, Debug, Default)]
pub struct CodeCheckConfigurationStore {
    value: Arc<RwLock<CodeCheckConfiguration>>,
}

impl CodeCheckConfigurationStore {
    pub fn new(value: CodeCheckConfiguration) -> Self {
        Self {
            value: Arc::new(RwLock::new(value)),
        }
    }

    pub fn value(&self) -> CodeCheckConfiguration {
        self.value.read().unwrap_or_else(|p| p.into_inner()).clone()
    }

    pub fn set_value(&self, value: CodeCheckConfiguration) {
        *self.value.write().unwrap_or_else(|p| p.into_inner()) = value;
    }
}

#[async_trait]
pub trait CodeChecking: Send + Sync {
    /// Checks the candidate at `root`. `php_files` are root-relative paths. Never fails:
    /// a tool that cannot run is reported as skipped.
    async fn check(
        &self,
        root: &Path,
        php_files: &[String],
        product: &Product,
        requires_php: Option<&str>,
    ) -> CodeCheckReport;
}

#[cfg(target_os = "macos")]
const PHP_NOT_INSTALLED: &str = "PHP is not installed (brew install php)";
#[cfg(all(unix, not(target_os = "macos")))]
const PHP_NOT_INSTALLED: &str = "PHP is not installed (sudo apt install php-cli)";
#[cfg(windows)]
const PHP_NOT_INSTALLED: &str = "PHP is not installed (winget install PHP.PHP.8.4, or add php.exe to PATH)";

#[cfg(target_os = "macos")]
const WP_NOT_INSTALLED: &str = "WP-CLI is not installed (brew install wp-cli)";
#[cfg(not(target_os = "macos"))]
const WP_NOT_INSTALLED: &str = "WP-CLI is not installed (see https://wp-cli.org/#installing)";

/// PHP syntax lint (`php -l`) and, when configured, WordPress.org's Plugin Check
/// (`wp plugin check`). Both only read the candidate.
#[derive(Clone)]
pub struct CodeCheckService {
    runner: Arc<dyn ProcessRunning>,
    php: Option<PathBuf>,
    wp: Option<PathBuf>,
    configuration: CodeCheckConfigurationStore,
}

impl Default for CodeCheckService {
    fn default() -> Self {
        Self::new()
    }
}

impl CodeCheckService {
    /// A real process runner, the first `php` and `wp` found (see
    /// [`crate::discovery::PHP_CANDIDATES`] / [`crate::discovery::WP_CANDIDATES`]) and an
    /// empty configuration.
    pub fn new() -> Self {
        Self {
            runner: Arc::new(ProcessRunner::default()),
            php: Self::find_php(),
            wp: find_executable(WP_CANDIDATES, "wp"),
            configuration: CodeCheckConfigurationStore::default(),
        }
    }

    pub fn find_php() -> Option<PathBuf> {
        find_executable(PHP_CANDIDATES, "php")
    }

    pub fn with_runner(mut self, runner: Arc<dyn ProcessRunning>) -> Self {
        self.runner = runner;
        self
    }

    pub fn with_php(mut self, php: Option<PathBuf>) -> Self {
        self.php = php;
        self
    }

    pub fn with_wp(mut self, wp: Option<PathBuf>) -> Self {
        self.wp = wp;
        self
    }

    pub fn with_configuration(mut self, configuration: CodeCheckConfigurationStore) -> Self {
        self.configuration = configuration;
        self
    }

    // MARK: PHP lint

    async fn lint(&self, root: &Path, files: &[String], requires_php: Option<&str>, report: &mut CodeCheckReport) {
        let Some(php) = &self.php else {
            report.php_lint_skipped = Some(PHP_NOT_INSTALLED.into());
            return;
        };
        let request =
            ProcessRequest::new(php, strings(&["-n", "-r", "echo PHP_VERSION;"])).timeout(Duration::from_secs(20));
        let version = match self.runner.run(request).await {
            Ok(version) if version.succeeded() && !version.standard_output.is_empty() => version,
            _ => {
                report.php_lint_skipped = Some(format!("php at {} did not run", php.display()));
                return;
            }
        };
        let php_version = version.stdout().trim().to_owned();
        report.php_version = Some(php_version.clone());
        if let Some(requires_php) = requires_php {
            if php_version::compare(&php_version, requires_php) < 0 {
                report.php_lint_skipped = Some(format!(
                    "Local PHP {php_version} is older than Requires PHP {requires_php}; newer syntax would be reported as errors"
                ));
                return;
            }
        }
        if files.is_empty() {
            return;
        }
        // PHP 8.3+ lints several files per invocation.
        let batch_size = if php_version::compare(&php_version, "8.3") >= 0 {
            100
        } else {
            1
        };
        for batch in files.chunks(batch_size) {
            let mut arguments = strings(&[
                "-n",
                "-d",
                "display_errors=1",
                "-d",
                "log_errors=0",
                "-d",
                "error_reporting=-1",
                "-l",
            ]);
            arguments.extend(batch.iter().cloned());
            let request = ProcessRequest::new(php, arguments)
                .working_directory(root)
                .timeout(Duration::from_secs(120));
            match self.runner.run(request).await {
                Ok(result) => {
                    report
                        .findings
                        .extend(Self::parse_lint(&format!("{}\n{}", result.stdout(), result.stderr())));
                    report.linted_files += batch.len() as i64;
                }
                Err(error) => {
                    report.php_lint_skipped = Some(format!("php -l failed: {error}"));
                    return;
                }
            }
        }
    }

    /// `Parse error: syntax error, … in includes/a.php on line 3`. Messages can contain " in "
    /// themselves, so the file is taken from the last one.
    pub fn parse_lint(output: &str) -> Vec<CodeCheckFinding> {
        // `R`: `^`/`$` also treat `\r\n` as a line break (PHP on Windows), like ICU.
        static PATTERN: LazyLock<Regex> = LazyLock::new(|| {
            #[allow(clippy::expect_used)]
            Regex::new(
                r"(?mR)^(?:PHP )?(Parse error|Fatal error|Deprecated|Warning):\s*(.+) in (.+?) on line (\d+)\s*$",
            )
            .expect("valid lint pattern")
        });
        let mut seen = HashSet::new();
        let mut findings = Vec::new();
        for captures in PATTERN.captures_iter(output) {
            let group = |i: usize| captures.get(i).map_or("", |m| m.as_str());
            let kind = group(1);
            let key = format!("{}:{}:{}", group(3), group(4), group(2));
            if !seen.insert(key) {
                continue;
            }
            findings.push(CodeCheckFinding::new(
                CodeCheckTool::PhpLint,
                if kind.ends_with("error") {
                    ValidationSeverity::Error
                } else {
                    ValidationSeverity::Warning
                },
                Some(group(3).to_owned()),
                group(4).parse().ok(),
                None,
                format!("{kind}: {}", group(2)),
            ));
        }
        findings
    }

    // MARK: Plugin Check

    async fn plugin_check(&self, root: &Path, product: &Product, report: &mut CodeCheckReport) {
        if product.product_type != ProductType::Plugin {
            return;
        }
        let path = self
            .configuration
            .value()
            .plugin_check_word_press_path
            .map(|p| p.trim_matches(is_swift_whitespace).to_owned())
            .filter(|p| !p.is_empty());
        let Some(path) = path else {
            report.plugin_check_skipped =
                Some("Not set up: choose a local WordPress install with Plugin Check in Settings".into());
            return;
        };
        let Some(wp) = &self.wp else {
            report.plugin_check_skipped = Some(WP_NOT_INSTALLED.into());
            return;
        };
        let mode = if product.current_version.is_none() {
            "new"
        } else {
            "update"
        };
        let arguments = vec![
            format!("--path={path}"),
            "plugin".into(),
            "check".into(),
            root.to_string_lossy().into_owned(),
            format!("--slug={}", product.slug),
            format!("--mode={mode}"),
            "--format=strict-json".into(),
            "--fields=file,line,type,code,message".into(),
        ];
        let request = ProcessRequest::new(wp, arguments).timeout(Duration::from_secs(300));
        match self.runner.run(request).await {
            Ok(result) => {
                if let Some(findings) = Self::parse_plugin_check(&result.stdout()) {
                    report.findings.extend(findings);
                    report.plugin_check_ran = true;
                } else if result.succeeded() {
                    report.plugin_check_ran = true; // nothing found
                } else {
                    let stderr = result.stderr();
                    let reason = stderr
                        .split('\n')
                        .find(|line| !line.is_empty())
                        .map_or_else(|| format!("exit code {}", result.exit_code), str::to_owned);
                    report.plugin_check_skipped = Some(format!("wp plugin check failed: {reason}"));
                }
            }
            Err(error) => {
                report.plugin_check_skipped = Some(format!("wp plugin check failed: {error}"));
            }
        }
    }

    /// Decodes the JSON array printed by `--format=strict-json`. WP-CLI and PHP may print
    /// notices around it, so each line that starts with `[` is tried. None when there is none.
    pub fn parse_plugin_check(output: &str) -> Option<Vec<CodeCheckFinding>> {
        #[derive(Deserialize)]
        struct Item {
            file: Option<String>,
            line: Option<i64>,
            #[serde(rename = "type")]
            kind: Option<String>,
            code: Option<String>,
            message: Option<String>,
        }
        let end = output.rfind(']')?;
        let bytes = output.as_bytes();
        let mut items: Option<Vec<Item>> = None;
        let mut line_start = 0;
        while items.is_none() && line_start < end {
            let trimmed = line_start
                + bytes[line_start..]
                    .iter()
                    .take_while(|b| **b == b' ' || **b == b'\t')
                    .count();
            if trimmed <= end && bytes.get(trimmed) == Some(&b'[') {
                items = serde_json::from_str(&output[trimmed..=end]).ok();
            }
            let Some(newline) = output[line_start..].find('\n') else {
                break;
            };
            line_start += newline + 1;
        }
        Some(
            items?
                .into_iter()
                .map(|item| {
                    let message = item
                        .message
                        .or_else(|| item.code.clone())
                        .unwrap_or_else(|| "Plugin Check finding".into());
                    CodeCheckFinding::new(
                        CodeCheckTool::PluginCheck,
                        if item.kind.is_some_and(|k| k.to_uppercase() == "ERROR") {
                            ValidationSeverity::Error
                        } else {
                            ValidationSeverity::Warning
                        },
                        item.file,
                        item.line,
                        item.code,
                        message,
                    )
                })
                .collect(),
        )
    }
}

#[async_trait]
impl CodeChecking for CodeCheckService {
    async fn check(
        &self,
        root: &Path,
        php_files: &[String],
        product: &Product,
        requires_php: Option<&str>,
    ) -> CodeCheckReport {
        let mut report = CodeCheckReport::default();
        self.lint(root, php_files, requires_php, &mut report).await;
        self.plugin_check(root, product, &mut report).await;
        report
    }
}

/// `CharacterSet.whitespaces`: spaces and tabs, not newlines.
fn is_swift_whitespace(c: char) -> bool {
    c == '\t'
        || (c.is_whitespace()
            && !matches!(
                c,
                '\n' | '\r' | '\u{0B}' | '\u{0C}' | '\u{85}' | '\u{2028}' | '\u{2029}'
            ))
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|v| (*v).to_owned()).collect()
}
