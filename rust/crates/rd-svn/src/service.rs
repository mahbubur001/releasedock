//! Port of `SVN/SVNService.swift`.

use crate::models::*;
use crate::process_runner::{
    ProcessRequest, ProcessResult, ProcessRunner, ProcessRunnerError, ProcessRunning, describe_duration,
    is_executable_file,
};
use crate::transcript::{SvnTranscribing, SvnTranscript};
use crate::xml_tree::XmlElement;
use async_trait::async_trait;
use rd_model::{Id, SwiftDate};
use rd_support::redact::redact;
use regex::Regex;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

static ERROR_CODE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"svn: (?:warning: )?([EW]\d{6}):").expect("valid error code pattern"));
static UPDATED_REVISION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:At|Updated to) revision (\d+)\.").expect("valid update pattern"));
static COMMITTED_REVISION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"Committed revision (\d+)\.").expect("valid commit pattern"));

#[async_trait]
pub trait SvnServicing: Send + Sync {
    async fn detect_installation(&self) -> Result<SvnInstallation, SvnError>;
    async fn info(&self, target: &str) -> Result<SvnInfo, SvnError>;
    /// True if the URL/path exists; false only when svn positively reports it missing.
    /// Network and authentication failures return an error, so they are never mistaken for "missing".
    async fn exists(&self, target: &str) -> Result<bool, SvnError>;
    async fn list(&self, url: &str, recursive: bool) -> Result<Vec<SvnListEntry>, SvnError>;
    async fn cat(&self, url: &str) -> Result<Vec<u8>, SvnError>;
    async fn checkout(&self, url: &str, path: &Path, depth: SvnDepth) -> Result<(), SvnError>;
    /// Updates and returns the working-copy revision.
    async fn update(&self, path: &Path, set_depth: Option<SvnDepth>) -> Result<i64, SvnError>;
    async fn status(&self, path: &Path, show_updates: bool) -> Result<Vec<SvnStatusEntry>, SvnError>;
    async fn diff(&self, path: &Path) -> Result<String, SvnError>;
    async fn add(&self, paths: &[PathBuf]) -> Result<(), SvnError>;
    async fn delete(&self, paths: &[PathBuf]) -> Result<(), SvnError>;
    async fn copy(&self, source: &Path, destination: &Path) -> Result<(), SvnError>;
    async fn revert(&self, path: &Path) -> Result<(), SvnError>;
    async fn cleanup(&self, path: &Path) -> Result<(), SvnError>;
    /// Marks conflicts as resolved with the given choice.
    async fn resolve(&self, paths: &[PathBuf], accept: SvnConflictChoice) -> Result<(), SvnError>;
    async fn set_property(&self, name: &str, value: &str, paths: &[PathBuf]) -> Result<(), SvnError>;
    /// Commits and returns the new revision. Returns [`SvnError::CommitOutcomeUncertain`] when
    /// the result cannot be known (timeout, interruption) — callers must not retry blindly.
    async fn commit(
        &self,
        paths: &[PathBuf],
        message: &str,
        credentials: Option<&SvnCredentials>,
    ) -> Result<i64, SvnError>;
    async fn log(&self, url: &str, limit: usize) -> Result<Vec<SvnLogEntry>, SvnError>;
    async fn export(&self, url: &str, path: &Path) -> Result<(), SvnError>;
    /// Whether svn has cached credentials whose realm matches the repository host.
    async fn has_cached_credentials(&self, repository_url: &str) -> Result<bool, SvnError>;
}

/// Drives the installed `svn` command-line client through a [`ProcessRunning`].
/// Every invocation uses `--non-interactive`, discrete arguments (no shell), and
/// `@`-escaped targets so file names like `logo@2x.png` are not read as peg revisions.
pub struct SvnService {
    runner: Arc<dyn ProcessRunning>,
    executable: Mutex<Option<PathBuf>>,
    preferred_executable: Option<PathBuf>,
    pub command_timeout: Duration,
    pub commit_timeout: Duration,
    /// HTTP timeout passed to svn (seconds) for reads, so a dead connection fails fast.
    /// svn's own default is 600 s.
    pub http_timeout_seconds: u64,
    /// HTTP timeout for commits. WordPress.org can take minutes to finish a large commit
    /// ("Committing transaction…"); a short timeout there turns a successful commit into
    /// E175012 and an uncertain outcome.
    pub commit_http_timeout_seconds: u64,
    /// Records commands run inside a [`SvnTranscript::with_scope`].
    pub transcript: Option<Arc<dyn SvnTranscribing>>,
}

impl Default for SvnService {
    fn default() -> Self {
        Self::new()
    }
}

impl SvnService {
    /// Places where `svn` is looked for, in order, before falling back to `PATH`.
    /// macOS (Homebrew on Apple silicon and Intel, MacPorts, system), Linux distributions
    /// (`/usr/bin/svn`), and on Windows the usual installers.
    #[cfg(not(windows))]
    pub const CANDIDATE_PATHS: &'static [&'static str] = &[
        "/opt/homebrew/bin/svn",
        "/usr/local/bin/svn",
        "/opt/local/bin/svn",
        "/usr/bin/svn",
    ];
    #[cfg(windows)]
    pub const CANDIDATE_PATHS: &'static [&'static str] = &[
        r"C:\Program Files\TortoiseSVN\bin\svn.exe",
        r"C:\Program Files\SlikSvn\bin\svn.exe",
        r"C:\Program Files\Subversion\bin\svn.exe",
        r"C:\Program Files (x86)\Subversion\bin\svn.exe",
    ];

    /// The Swift defaults: a real [`ProcessRunner`], 300 s per command, 1800 s per commit,
    /// a 120 s HTTP timeout for reads and 1800 s for commits, no transcript.
    pub fn new() -> Self {
        Self {
            runner: Arc::new(ProcessRunner::default()),
            executable: Mutex::new(None),
            preferred_executable: None,
            command_timeout: Duration::from_secs(300),
            commit_timeout: Duration::from_secs(1800),
            http_timeout_seconds: 120,
            commit_http_timeout_seconds: 1800,
            transcript: None,
        }
    }

    pub fn with_runner(mut self, runner: Arc<dyn ProcessRunning>) -> Self {
        self.runner = runner;
        self
    }

    /// Tries `executable` before the standard locations.
    pub fn with_executable(mut self, executable: impl Into<PathBuf>) -> Self {
        self.preferred_executable = Some(executable.into());
        self
    }

    pub fn with_command_timeout(mut self, timeout: Duration) -> Self {
        self.command_timeout = timeout;
        self
    }

    pub fn with_commit_timeout(mut self, timeout: Duration) -> Self {
        self.commit_timeout = timeout;
        self
    }

    pub fn with_http_timeout_seconds(mut self, seconds: u64) -> Self {
        self.http_timeout_seconds = seconds;
        self
    }

    pub fn with_commit_http_timeout_seconds(mut self, seconds: u64) -> Self {
        self.commit_http_timeout_seconds = seconds;
        self
    }

    pub fn with_transcript(mut self, transcript: Arc<dyn SvnTranscribing>) -> Self {
        self.transcript = Some(transcript);
        self
    }

    // MARK: Installation

    /// Every place `svn` may be: the preferred executable, [`Self::CANDIDATE_PATHS`], then
    /// `svn` (`svn.exe`) on the parent process's `PATH`.
    pub fn candidate_executables(preferred: Option<&Path>) -> Vec<PathBuf> {
        let mut candidates: Vec<PathBuf> = preferred.map(Path::to_path_buf).into_iter().collect();
        candidates.extend(Self::CANDIDATE_PATHS.iter().map(PathBuf::from));
        if let Ok(found) = which::which("svn") {
            if !candidates.contains(&found) {
                candidates.push(found);
            }
        }
        candidates
    }

    fn cached_executable(&self) -> Option<PathBuf> {
        self.executable.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    async fn svn_executable(&self) -> Result<PathBuf, SvnError> {
        if let Some(cached) = self.cached_executable() {
            return Ok(cached);
        }
        Ok(self.detect_installation().await?.executable)
    }

    // MARK: Running

    /// Appends `@` to targets containing `@`, the documented way to stop svn treating
    /// the text after the last `@` as a peg revision.
    pub fn target(raw: &str) -> String {
        if raw.contains('@') {
            format!("{raw}@")
        } else {
            raw.to_owned()
        }
    }

    /// [`Self::target`] for a local path.
    pub fn path_target(path: &Path) -> String {
        Self::target(&path.to_string_lossy())
    }

    pub fn global_options(http_timeout: u64) -> Vec<String> {
        vec![
            "--non-interactive".to_owned(),
            "--config-option".to_owned(),
            format!("servers:global:http-timeout={http_timeout}"),
        ]
    }

    async fn svn(&self, arguments: Vec<String>) -> Result<ProcessResult, SvnError> {
        self.svn_with(arguments, None, None, None, true).await
    }

    async fn svn_with(
        &self,
        arguments: Vec<String>,
        credentials: Option<&SvnCredentials>,
        timeout: Option<Duration>,
        http_timeout: Option<u64>,
        throw_on_failure: bool,
    ) -> Result<ProcessResult, SvnError> {
        let executable = self.svn_executable().await?;
        let mut args = arguments.clone();
        args.extend(Self::global_options(http_timeout.unwrap_or(self.http_timeout_seconds)));
        let mut request = ProcessRequest::new(executable, args);
        request.timeout = timeout.unwrap_or(self.command_timeout);
        if let Some(credentials) = credentials {
            // Password via stdin (requires --non-interactive); --no-auth-cache keeps it out of ~/.subversion.
            request.arguments.extend([
                "--username".to_owned(),
                credentials.username.clone(),
                "--password-from-stdin".to_owned(),
                "--no-auth-cache".to_owned(),
            ]);
            request.standard_input = Some(format!("{}\n", credentials.password).into_bytes());
        }
        let start = Instant::now();
        // As shown in the transcript: no global options, and the password only as --password-from-stdin.
        let mut shown = arguments;
        if let Some(credentials) = credentials {
            shown.extend([
                "--username".to_owned(),
                credentials.username.clone(),
                "--password-from-stdin".to_owned(),
            ]);
        }
        let result = match self.runner.run(request).await {
            Ok(result) => result,
            Err(error) => {
                self.transcribe(&shown, error.partial_result(), Some(&error.to_string()), start);
                return Err(error.into());
            }
        };
        self.transcribe(&shown, Some(&result), None, start);
        log::debug!(
            target: "svn",
            "svn {} exit={} in {:.2}s",
            shown.first().map(String::as_str).unwrap_or(""),
            result.exit_code,
            start.elapsed().as_secs_f64()
        );
        if !result.succeeded() && throw_on_failure {
            return Err(Self::error(&result));
        }
        Ok(result)
    }

    fn transcribe(&self, arguments: &[String], result: Option<&ProcessResult>, failure: Option<&str>, start: Instant) {
        let (Some(transcript), Some(scope)) = (&self.transcript, SvnTranscript::scope()) else {
            return;
        };
        transcript.record(SvnTranscript::entry(
            &scope,
            arguments,
            None,
            result,
            failure,
            start.elapsed().as_secs_f64(),
            SwiftDate::now(),
        ));
    }

    pub fn error_codes(stderr: &str) -> Vec<String> {
        ERROR_CODE.captures_iter(stderr).map(|c| c[1].to_owned()).collect()
    }

    pub fn error(result: &ProcessResult) -> SvnError {
        let stderr = redact(&result.stderr());
        let codes = Self::error_codes(&stderr);
        let lines: Vec<&str> = stderr
            .split(['\n', '\r', '\u{0B}', '\u{0C}', '\u{85}', '\u{2028}', '\u{2029}'])
            .map(|line| line.trim_matches(|c: char| c.is_whitespace() && !is_newline(c)))
            .filter(|line| !line.is_empty())
            .collect();
        let message = lines[lines.len().saturating_sub(4)..].join(" ");
        SvnError::CommandFailed {
            kind: SvnErrorKind::classify(&codes, &stderr),
            codes,
            message,
            command: result.display_command.clone(),
        }
    }

    fn xml(result: &ProcessResult) -> Result<XmlElement, SvnError> {
        XmlElement::parse(&result.standard_output)
            .map_err(|_| SvnError::UnexpectedOutput(format!("could not parse XML from {}", result.display_command)))
    }

    pub fn parse_info(entry: &XmlElement) -> SvnInfo {
        let commit = entry.child("commit");
        let repository = entry.child("repository");
        let text = |element: Option<&XmlElement>| element.map(|e| e.trimmed_text().to_owned());
        SvnInfo {
            url: text(entry.child("url")).unwrap_or_default(),
            repository_root: text(repository.and_then(|r| r.child("root"))).unwrap_or_default(),
            repository_uuid: text(repository.and_then(|r| r.child("uuid"))),
            revision: int_attribute(entry, "revision").unwrap_or(0),
            kind: entry
                .attribute("kind")
                .and_then(SvnNodeKind::from_raw)
                .unwrap_or(SvnNodeKind::Unknown),
            last_changed_revision: commit.and_then(|c| int_attribute(c, "revision")),
            last_changed_author: text(commit.and_then(|c| c.child("author"))),
            last_changed_date: text(commit.and_then(|c| c.child("date"))),
            working_copy_root: text(entry.child("wc-info").and_then(|w| w.child("wcroot-abspath"))),
        }
    }

    pub fn parse_status(root: &XmlElement) -> Vec<SvnStatusEntry> {
        root.descendants("entry")
            .into_iter()
            .filter_map(|entry| {
                let path = entry.attribute("path")?;
                let wc = entry.child("wc-status")?;
                let repos = entry.child("repos-status");
                let status = |e: &XmlElement, name| e.attribute(name).and_then(SvnItemStatus::from_raw);
                Some(SvnStatusEntry {
                    path: path.to_owned(),
                    item: status(wc, "item").unwrap_or(SvnItemStatus::None),
                    props: status(wc, "props").unwrap_or(SvnItemStatus::None),
                    copied: wc.attribute("copied") == Some("true"),
                    is_tree_conflict: wc.attribute("tree-conflicted") == Some("true"),
                    repository_item: repos.and_then(|r| status(r, "item")),
                    repository_props: repos.and_then(|r| status(r, "props")),
                    working_revision: int_attribute(wc, "revision"),
                })
            })
            .collect()
    }

    pub fn parse_list(root: &XmlElement) -> Vec<SvnListEntry> {
        root.descendants("entry")
            .into_iter()
            .map(|entry| SvnListEntry {
                name: entry
                    .child("name")
                    .map(|n| n.trimmed_text().to_owned())
                    .unwrap_or_default(),
                kind: entry
                    .attribute("kind")
                    .and_then(SvnNodeKind::from_raw)
                    .unwrap_or(SvnNodeKind::Unknown),
                size: entry.child("size").and_then(|s| s.trimmed_text().parse().ok()),
                commit_revision: entry.child("commit").and_then(|c| int_attribute(c, "revision")),
            })
            .collect()
    }

    pub fn parse_log(root: &XmlElement) -> Vec<SvnLogEntry> {
        root.descendants("logentry")
            .into_iter()
            .map(|entry| SvnLogEntry {
                revision: int_attribute(entry, "revision").unwrap_or(0),
                author: entry.child("author").map(|a| a.trimmed_text().to_owned()),
                date: entry.child("date").map(|d| d.trimmed_text().to_owned()),
                message: entry.child("msg").map(|m| m.text.clone()).unwrap_or_default(),
                changed_paths: entry
                    .child("paths")
                    .map(|p| {
                        p.all("path")
                            .into_iter()
                            .map(|path| path.trimmed_text().to_owned())
                            .collect()
                    })
                    .unwrap_or_default(),
                changed_path_actions: entry
                    .child("paths")
                    .map(|p| {
                        p.all("path")
                            .into_iter()
                            .map(|path| path.attribute("action").unwrap_or("M").to_owned())
                            .collect()
                    })
                    .unwrap_or_default(),
            })
            .collect()
    }

    pub fn committed_revision(stdout: &str) -> Option<i64> {
        COMMITTED_REVISION.captures(stdout).and_then(|c| c[1].parse().ok())
    }

    /// Passes long path lists through a `--targets` file to avoid argv limits.
    async fn with_targets(&self, paths: &[PathBuf], arguments: &[&str]) -> Result<(), SvnError> {
        if paths.is_empty() {
            return Ok(());
        }
        let file = TemporaryFile::new("releasedock-targets")?;
        let content = paths
            .iter()
            .map(|p| Self::path_target(p))
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        file.write(content.as_bytes())?;
        let mut args: Vec<String> = arguments.iter().map(|a| (*a).to_owned()).collect();
        args.extend(["--targets".to_owned(), file.path.to_string_lossy().into_owned()]);
        self.svn(args).await?;
        Ok(())
    }
}

fn is_newline(c: char) -> bool {
    matches!(
        c,
        '\n' | '\r' | '\u{0B}' | '\u{0C}' | '\u{85}' | '\u{2028}' | '\u{2029}'
    )
}

/// `Int(attribute)`: the attribute as a whole number, `None` when absent or not a number.
fn int_attribute(element: &XmlElement, name: &str) -> Option<i64> {
    element.attribute(name).and_then(|v| v.parse().ok())
}

fn strings<const N: usize>(values: [&str; N]) -> Vec<String> {
    values.iter().map(|v| (*v).to_owned()).collect()
}

/// Splits `scheme://[user@]host[:port]/…` into scheme and host, like `URL.scheme` / `URL.host`.
fn scheme_and_host(url: &str) -> (Option<&str>, Option<&str>) {
    let Some((scheme, rest)) = url.split_once("://") else {
        return (None, None);
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host_port = authority.rsplit_once('@').map_or(authority, |(_, host)| host);
    let host = if let Some(bracketed) = host_port.strip_prefix('[') {
        bracketed.split(']').next().unwrap_or("")
    } else {
        host_port.split(':').next().unwrap_or("")
    };
    (Some(scheme), (!host.is_empty()).then_some(host))
}

/// A file in the system temporary folder, removed when dropped.
struct TemporaryFile {
    path: PathBuf,
}

impl TemporaryFile {
    fn new(prefix: &str) -> std::io::Result<Self> {
        Ok(Self {
            path: std::env::temp_dir().join(format!("{prefix}-{}.txt", Id::new())),
        })
    }

    fn write(&self, data: &[u8]) -> std::io::Result<()> {
        std::fs::write(&self.path, data)
    }
}

impl Drop for TemporaryFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[async_trait]
impl SvnServicing for SvnService {
    async fn detect_installation(&self) -> Result<SvnInstallation, SvnError> {
        for path in Self::candidate_executables(self.preferred_executable.as_deref()) {
            if !is_executable_file(&path) {
                continue;
            }
            let request =
                ProcessRequest::new(path.clone(), strings(["--version", "--quiet"])).timeout(Duration::from_secs(20));
            let result = self.runner.run(request).await?;
            if !result.succeeded() {
                continue;
            }
            let version = result.stdout().trim().to_owned();
            *self.executable.lock().unwrap_or_else(|p| p.into_inner()) = Some(path.clone());
            return Ok(SvnInstallation {
                executable: path,
                version,
            });
        }
        Err(SvnError::NotInstalled)
    }

    // MARK: Queries

    async fn info(&self, target: &str) -> Result<SvnInfo, SvnError> {
        let result = self
            .svn(vec!["info".into(), "--xml".into(), Self::target(target)])
            .await?;
        let root = Self::xml(&result)?;
        let entry = root
            .descendants("entry")
            .into_iter()
            .next()
            .ok_or_else(|| SvnError::UnexpectedOutput("svn info returned no entry".into()))?;
        Ok(Self::parse_info(entry))
    }

    async fn exists(&self, target: &str) -> Result<bool, SvnError> {
        match self.info(target).await {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == Some(SvnErrorKind::NotFound) => Ok(false),
            Err(error) => Err(error),
        }
    }

    async fn list(&self, url: &str, recursive: bool) -> Result<Vec<SvnListEntry>, SvnError> {
        let mut args = strings(["list", "--xml"]);
        if recursive {
            args.push("--recursive".into());
        }
        args.push(Self::target(url));
        let result = self.svn(args).await?;
        Ok(Self::parse_list(&Self::xml(&result)?))
    }

    async fn cat(&self, url: &str) -> Result<Vec<u8>, SvnError> {
        Ok(self.svn(vec!["cat".into(), Self::target(url)]).await?.standard_output)
    }

    async fn status(&self, path: &Path, show_updates: bool) -> Result<Vec<SvnStatusEntry>, SvnError> {
        // --no-ignore: files matching svn's global-ignores (e.g. *.a, *.so, .DS_Store) would
        // otherwise be invisible and silently left out of the release.
        let mut args = strings(["status", "--xml", "--no-ignore"]);
        if show_updates {
            args.push("--show-updates".into());
        }
        args.push(Self::path_target(path));
        let result = self.svn(args).await?;
        Ok(Self::parse_status(&Self::xml(&result)?))
    }

    async fn diff(&self, path: &Path) -> Result<String, SvnError> {
        // Text diff only; binary files are reported by svn as "Cannot display".
        Ok(self
            .svn(vec!["diff".into(), "--internal-diff".into(), Self::path_target(path)])
            .await?
            .stdout())
    }

    async fn log(&self, url: &str, limit: usize) -> Result<Vec<SvnLogEntry>, SvnError> {
        let args = vec![
            "log".into(),
            "--xml".into(),
            "--verbose".into(),
            "--limit".into(),
            limit.to_string(),
            Self::target(url),
        ];
        let result = self.svn(args).await?;
        Ok(Self::parse_log(&Self::xml(&result)?))
    }

    async fn has_cached_credentials(&self, repository_url: &str) -> Result<bool, SvnError> {
        let (scheme, Some(host)) = scheme_and_host(repository_url) else {
            return Ok(false);
        };
        let pattern = format!("<{}://{host}:*", scheme.unwrap_or("https"));
        let result = self.svn(vec!["auth".into(), pattern]).await?;
        // `svn auth` prints "Credentials cache in '…' contains N credentials" and one block per match.
        Ok(result.stdout().contains("Username:"))
    }

    // MARK: Working copy

    async fn checkout(&self, url: &str, path: &Path, depth: SvnDepth) -> Result<(), SvnError> {
        let args = vec![
            "checkout".into(),
            "--depth".into(),
            depth.raw_value().into(),
            Self::target(url),
            Self::path_target(path),
        ];
        self.svn(args).await?;
        Ok(())
    }

    async fn update(&self, path: &Path, set_depth: Option<SvnDepth>) -> Result<i64, SvnError> {
        let mut args = strings(["update"]);
        if let Some(depth) = set_depth {
            args.extend(["--set-depth".into(), depth.raw_value().into()]);
        }
        args.push(Self::path_target(path));
        let result = self.svn(args).await?;
        let out = result.stdout();
        if let Some(revision) = UPDATED_REVISION
            .captures_iter(&out)
            .last()
            .and_then(|c| c[1].parse().ok())
        {
            return Ok(revision);
        }
        Ok(self.info(&path.to_string_lossy()).await?.revision)
    }

    async fn add(&self, paths: &[PathBuf]) -> Result<(), SvnError> {
        // --parents adds intermediate directories; --no-ignore so global-ignores never drop files.
        self.with_targets(
            paths,
            &["add", "--parents", "--no-ignore", "--force", "--depth", "infinity"],
        )
        .await
    }

    async fn delete(&self, paths: &[PathBuf]) -> Result<(), SvnError> {
        self.with_targets(paths, &["delete", "--force"]).await
    }

    async fn copy(&self, source: &Path, destination: &Path) -> Result<(), SvnError> {
        self.svn(vec![
            "copy".into(),
            "--parents".into(),
            Self::path_target(source),
            Self::path_target(destination),
        ])
        .await?;
        Ok(())
    }

    async fn revert(&self, path: &Path) -> Result<(), SvnError> {
        self.svn(vec!["revert".into(), "--recursive".into(), Self::path_target(path)])
            .await?;
        Ok(())
    }

    async fn cleanup(&self, path: &Path) -> Result<(), SvnError> {
        self.svn(vec!["cleanup".into(), Self::path_target(path)]).await?;
        Ok(())
    }

    async fn resolve(&self, paths: &[PathBuf], accept: SvnConflictChoice) -> Result<(), SvnError> {
        self.with_targets(paths, &["resolve", "--accept", accept.raw_value()])
            .await
    }

    async fn set_property(&self, name: &str, value: &str, paths: &[PathBuf]) -> Result<(), SvnError> {
        self.with_targets(paths, &["propset", name, value]).await
    }

    async fn commit(
        &self,
        paths: &[PathBuf],
        message: &str,
        credentials: Option<&SvnCredentials>,
    ) -> Result<i64, SvnError> {
        // The message goes into a file: stdin may be carrying the password.
        let message_file = TemporaryFile::new("releasedock-commit")?;
        message_file.write(message.as_bytes())?;

        let mut args = vec![
            "commit".to_owned(),
            "--file".to_owned(),
            message_file.path.to_string_lossy().into_owned(),
            "--encoding".to_owned(),
            "UTF-8".to_owned(),
        ];
        args.extend(paths.iter().map(|p| Self::path_target(p)));
        let outcome = self
            .svn_with(
                args,
                credentials,
                Some(self.commit_timeout),
                Some(self.commit_http_timeout_seconds),
                false,
            )
            .await;
        let result = match outcome {
            Ok(result) => result,
            Err(SvnError::Process(ProcessRunnerError::TimedOut(partial))) => {
                if let Some(revision) = Self::committed_revision(&partial.stdout()) {
                    return Ok(revision);
                }
                return Err(SvnError::CommitOutcomeUncertain(format!(
                    "timed out after {}",
                    describe_duration(partial.duration)
                )));
            }
            Err(SvnError::Process(ProcessRunnerError::Cancelled(partial))) => {
                if let Some(revision) = Self::committed_revision(&partial.stdout()) {
                    return Ok(revision);
                }
                return Err(SvnError::CommitOutcomeUncertain("interrupted".into()));
            }
            Err(error) => return Err(error),
        };
        // "Committed revision N." is printed only after the server accepted the commit,
        // even if a post-commit step then reports an error.
        if let Some(revision) = Self::committed_revision(&result.stdout()) {
            return Ok(revision);
        }
        if result.succeeded() {
            if result.stdout().trim().is_empty() {
                return Err(SvnError::UnexpectedOutput(
                    "svn commit reported nothing to commit".into(),
                ));
            }
            return Err(SvnError::CommitOutcomeUncertain(
                "could not read the committed revision".into(),
            ));
        }
        let error = Self::error(&result);
        use SvnErrorKind::*;
        match error.kind() {
            // Rejected before the transaction was committed: definitely not in the repository.
            Some(
                AuthenticationRequired
                | AuthenticationFailed
                | Forbidden
                | HookRejected
                | OutOfDate
                | Conflict
                | AlreadyExists
                | WorkingCopyLocked
                | NotAWorkingCopy
                | Certificate
                | NotFound,
            ) => Err(error),
            // Network failures and anything unrecognised may have happened after the server
            // committed; never assume either way.
            _ => Err(SvnError::CommitOutcomeUncertain(error.to_string())),
        }
    }

    async fn export(&self, url: &str, path: &Path) -> Result<(), SvnError> {
        self.svn(vec![
            "export".into(),
            "--force".into(),
            Self::target(url),
            Self::path_target(path),
        ])
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scheme_and_host_like_foundation_url() {
        assert_eq!(
            scheme_and_host("https://plugins.svn.wordpress.org/my-plugin"),
            (Some("https"), Some("plugins.svn.wordpress.org"))
        );
        assert_eq!(
            scheme_and_host("svn://user@example.com:3690/repo"),
            (Some("svn"), Some("example.com"))
        );
        assert_eq!(scheme_and_host("http://[::1]:8080/x"), (Some("http"), Some("::1")));
        assert_eq!(scheme_and_host("file:///tmp/repo"), (Some("file"), None));
        assert_eq!(scheme_and_host("not a url"), (None, None));
    }

    #[test]
    fn error_message_keeps_the_last_four_lines() {
        let result = ProcessResult {
            exit_code: 1,
            standard_output: Vec::new(),
            standard_error:
                b"svn: E1\n\n  svn: E2  \r\nsvn: E3\nsvn: E4\nsvn: E170013: Unable to connect --password hunter2\n"
                    .to_vec(),
            duration: Duration::ZERO,
            display_command: "svn info".into(),
        };
        let SvnError::CommandFailed {
            kind,
            codes,
            message,
            command,
        } = SvnService::error(&result)
        else {
            panic!("expected commandFailed")
        };
        assert_eq!(kind, SvnErrorKind::Network);
        assert_eq!(codes, vec!["E170013"]);
        assert_eq!(
            message,
            "svn: E2 svn: E3 svn: E4 svn: E170013: Unable to connect --password <redacted>"
        );
        assert_eq!(command, "svn info");
    }
}
