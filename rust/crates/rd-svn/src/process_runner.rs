//! Port of `Support/ProcessRunner.swift`: runs external executables without a shell,
//! with stdin data, a timeout and cooperative cancellation.

use async_trait::async_trait;
use std::collections::{BTreeMap, HashSet};
use std::fmt;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, Command};
use tokio_util::sync::CancellationToken;

tokio::task_local! {
    /// The cancellation token of the current task, set with [`with_cancellation`]. Stands in
    /// for Swift's implicit task cancellation.
    static CANCELLATION: CancellationToken;
}

/// Runs `future` so that every process started inside it is terminated (and reports
/// [`ProcessRunnerError::Cancelled`] with its partial output) when `token` is cancelled.
///
/// This is the Rust counterpart of cancelling the Swift `Task` that runs a command.
/// Dropping the future instead also stops the child (it is killed immediately), but then
/// no partial result can be returned.
pub async fn with_cancellation<F: Future>(token: CancellationToken, future: F) -> F::Output {
    CANCELLATION.scope(token, future).await
}

fn current_cancellation() -> Option<CancellationToken> {
    CANCELLATION.try_with(CancellationToken::clone).ok()
}

/// A request to run an external executable. Arguments are always passed as a
/// discrete list straight to the OS (`execve` / `CreateProcess`); no shell is involved, so
/// user-controlled strings can never be interpreted as shell syntax.
#[derive(Clone)]
pub struct ProcessRequest {
    pub executable: PathBuf,
    pub arguments: Vec<String>,
    pub working_directory: Option<PathBuf>,
    pub environment: BTreeMap<String, String>,
    /// Bytes written to the child's stdin, then stdin is closed. Used for
    /// `svn --password-from-stdin` so secrets never appear in `ps` output.
    pub standard_input: Option<Vec<u8>>,
    pub timeout: Duration,
    /// Arguments at these indices are replaced with `<redacted>` in descriptions and logs.
    pub redacted_argument_indices: HashSet<usize>,
}

impl ProcessRequest {
    /// A request with the Swift defaults: [`ProcessRequest::default_environment`] for the
    /// current user's home folder, no stdin, a 120 s timeout and nothing redacted.
    pub fn new(executable: impl Into<PathBuf>, arguments: Vec<String>) -> Self {
        Self {
            executable: executable.into(),
            arguments,
            working_directory: None,
            environment: Self::default_environment(&home_directory()),
            standard_input: None,
            timeout: Duration::from_secs(120),
            redacted_argument_indices: HashSet::new(),
        }
    }

    pub fn working_directory(mut self, directory: impl Into<PathBuf>) -> Self {
        self.working_directory = Some(directory.into());
        self
    }

    pub fn standard_input(mut self, input: impl Into<Vec<u8>>) -> Self {
        self.standard_input = Some(input.into());
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn redacted_argument_indices(mut self, indices: impl IntoIterator<Item = usize>) -> Self {
        self.redacted_argument_indices = indices.into_iter().collect();
        self
    }

    /// A minimal, predictable environment. The parent environment is not inherited
    /// so stray variables (SVN_EDITOR, GIT_DIR, proxies set by other tools…) cannot
    /// change behaviour. A UTF-8 locale is required for svn to handle non-ASCII paths.
    ///
    /// On Windows a few system variables are passed through as well, because programs
    /// there cannot start without `SystemRoot` and svn keeps its configuration under
    /// `%APPDATA%`.
    pub fn default_environment(home: &str) -> BTreeMap<String, String> {
        let mut environment = BTreeMap::new();
        #[cfg(not(windows))]
        environment.insert(
            "PATH".to_owned(),
            "/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin:/usr/local/bin".to_owned(),
        );
        #[cfg(windows)]
        for name in [
            "PATH",
            "PATHEXT",
            "SystemRoot",
            "SystemDrive",
            "windir",
            "TEMP",
            "TMP",
            "USERPROFILE",
            "APPDATA",
            "LOCALAPPDATA",
            "ComSpec",
        ] {
            if let Ok(value) = std::env::var(name) {
                environment.insert(name.to_owned(), value);
            }
        }
        environment.insert("HOME".to_owned(), home.to_owned());
        environment.insert("LANG".to_owned(), "en_US.UTF-8".to_owned());
        environment.insert("LC_ALL".to_owned(), "en_US.UTF-8".to_owned());
        environment
    }

    /// Human-readable command line with redacted arguments, for logs and diagnostics only.
    pub fn display_command(&self) -> String {
        let mut parts = vec![self.executable.to_string_lossy().into_owned()];
        parts.extend(self.arguments.iter().enumerate().map(|(index, argument)| {
            if self.redacted_argument_indices.contains(&index) {
                "<redacted>".to_owned()
            } else {
                argument.clone()
            }
        }));
        parts.join(" ")
    }
}

/// The current user's home folder (`NSHomeDirectory()` in Swift).
pub fn home_directory() -> String {
    #[cfg(windows)]
    let variables = ["USERPROFILE", "HOME"];
    #[cfg(not(windows))]
    let variables = ["HOME"];
    variables
        .iter()
        .find_map(|name| std::env::var(name).ok().filter(|v| !v.is_empty()))
        .unwrap_or_default()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessResult {
    pub exit_code: i32,
    pub standard_output: Vec<u8>,
    pub standard_error: Vec<u8>,
    pub duration: Duration,
    pub display_command: String,
}

impl ProcessResult {
    pub fn succeeded(&self) -> bool {
        self.exit_code == 0
    }

    /// stdout decoded as UTF-8, invalid bytes replaced (like `String(decoding:as:)`).
    pub fn stdout(&self) -> String {
        String::from_utf8_lossy(&self.standard_output).into_owned()
    }

    pub fn stderr(&self) -> String {
        String::from_utf8_lossy(&self.standard_error).into_owned()
    }
}

/// Formats a duration like Swift's `Duration.description` (`"0.3012 seconds"`), so
/// messages built from it read the same in both apps.
pub fn describe_duration(duration: Duration) -> String {
    format!("{:?} seconds", duration.as_secs_f64())
}

#[derive(Clone, Debug, thiserror::Error)]
pub enum ProcessRunnerError {
    #[error("Executable not found: {0}")]
    ExecutableNotFound(String),
    #[error("Failed to launch {command}: {reason}")]
    LaunchFailed { command: String, reason: String },
    /// The process exceeded its timeout and was terminated. Partial output is preserved
    /// because callers (notably SVN commit) must treat the outcome as *uncertain*.
    #[error("Timed out after {}: {}", describe_duration(.0.duration), .0.display_command)]
    TimedOut(ProcessResult),
    #[error("Cancelled: {}", .0.display_command)]
    Cancelled(ProcessResult),
}

impl ProcessRunnerError {
    /// The partial result of a process that was stopped (timeout or cancellation).
    pub fn partial_result(&self) -> Option<&ProcessResult> {
        match self {
            Self::TimedOut(result) | Self::Cancelled(result) => Some(result),
            _ => None,
        }
    }
}

#[async_trait]
pub trait ProcessRunning: Send + Sync {
    async fn run(&self, request: ProcessRequest) -> Result<ProcessResult, ProcessRunnerError>;
}

/// True when `path` is a regular file the current user may execute
/// (`FileManager.isExecutableFile(atPath:)`).
pub fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        metadata.is_file()
    }
}

/// Runs child processes with timeout and cooperative cancellation support.
#[derive(Clone, Debug)]
pub struct ProcessRunner {
    /// Grace period between SIGTERM and SIGKILL.
    pub kill_grace_period: Duration,
}

impl Default for ProcessRunner {
    fn default() -> Self {
        Self {
            kill_grace_period: Duration::from_secs(5),
        }
    }
}

impl ProcessRunner {
    pub fn new(kill_grace_period: Duration) -> Self {
        Self { kill_grace_period }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StopReason {
    TimedOut,
    Cancelled,
}

#[async_trait]
impl ProcessRunning for ProcessRunner {
    async fn run(&self, request: ProcessRequest) -> Result<ProcessResult, ProcessRunnerError> {
        if !is_executable_file(&request.executable) {
            return Err(ProcessRunnerError::ExecutableNotFound(
                request.executable.to_string_lossy().into_owned(),
            ));
        }
        let display_command = request.display_command();

        let mut command = Command::new(&request.executable);
        command
            .args(&request.arguments)
            .env_clear()
            .envs(&request.environment)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(if request.standard_input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            // A dropped (abandoned) run must not leave the child behind.
            .kill_on_drop(true);
        if let Some(directory) = &request.working_directory {
            command.current_dir(directory);
        }

        let start = Instant::now();
        let mut child = command.spawn().map_err(|error| ProcessRunnerError::LaunchFailed {
            command: display_command.clone(),
            reason: error.to_string(),
        })?;

        if let (Some(mut stdin), Some(input)) = (child.stdin.take(), request.standard_input) {
            tokio::spawn(async move {
                let _ = stdin.write_all(&input).await;
                let _ = stdin.shutdown().await;
            });
        }

        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let read = async { tokio::join!(read_all(stdout), read_all(stderr)) };
        let wait = supervise(
            &mut child,
            request.timeout,
            self.kill_grace_period,
            current_cancellation(),
        );
        let ((out, err), (status, reason)) = tokio::join!(read, wait);

        let result = ProcessResult {
            exit_code: status.as_ref().map(exit_code).unwrap_or(-1),
            standard_output: out,
            standard_error: err,
            duration: start.elapsed(),
            display_command: display_command.clone(),
        };
        match (reason, status) {
            (Some(StopReason::TimedOut), _) => Err(ProcessRunnerError::TimedOut(result)),
            (Some(StopReason::Cancelled), _) => Err(ProcessRunnerError::Cancelled(result)),
            (None, Ok(_)) => Ok(result),
            (None, Err(error)) => Err(ProcessRunnerError::LaunchFailed {
                command: display_command,
                reason: error.to_string(),
            }),
        }
    }
}

async fn read_all<R: AsyncRead + Unpin>(reader: Option<R>) -> Vec<u8> {
    let mut data = Vec::new();
    if let Some(mut reader) = reader {
        let _ = reader.read_to_end(&mut data).await;
    }
    data
}

/// Waits for the child, terminating it on timeout or cancellation. Only the first of the
/// two takes effect, and only while the child is still running.
async fn supervise(
    child: &mut Child,
    timeout: Duration,
    grace: Duration,
    cancellation: Option<CancellationToken>,
) -> (std::io::Result<ExitStatus>, Option<StopReason>) {
    let cancelled = async {
        match &cancellation {
            Some(token) => token.cancelled().await,
            None => std::future::pending().await,
        }
    };
    let reason = tokio::select! {
        biased;
        status = child.wait() => return (status, None),
        _ = tokio::time::sleep(timeout) => StopReason::TimedOut,
        _ = cancelled => StopReason::Cancelled,
    };
    // Already exited but not yet reaped: it finished on its own.
    if let Ok(Some(status)) = child.try_wait() {
        return (Ok(status), None);
    }
    terminate(child);
    let status = tokio::select! {
        status = child.wait() => status,
        _ = tokio::time::sleep(grace) => {
            let _ = child.start_kill();
            child.wait().await
        }
    };
    (status, Some(reason))
}

/// Asks the child to stop: SIGTERM on Unix, `TerminateProcess` on Windows.
fn terminate(child: &mut Child) {
    #[cfg(unix)]
    {
        use nix::sys::signal::{Signal, kill};
        use nix::unistd::Pid;
        if let Some(pid) = child.id().and_then(|id| i32::try_from(id).ok()) {
            if kill(Pid::from_raw(pid), Signal::SIGTERM).is_ok() {
                return;
            }
        }
    }
    let _ = child.start_kill();
}

/// The exit code, or the signal number when the process was killed by a signal
/// (what `Process.terminationStatus` reports).
fn exit_code(status: &ExitStatus) -> i32 {
    if let Some(code) = status.code() {
        return code;
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return signal;
        }
    }
    -1
}

/// Never prints stdin (it may carry a password) or redacted arguments.
impl fmt::Debug for ProcessRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProcessRequest")
            .field("command", &self.display_command())
            .field("working_directory", &self.working_directory)
            .field(
                "standard_input",
                &self
                    .standard_input
                    .as_ref()
                    .map(|input| format!("<{} bytes>", input.len())),
            )
            .field("timeout", &self.timeout)
            .finish_non_exhaustive()
    }
}

impl fmt::Display for ProcessRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.display_command())
    }
}
