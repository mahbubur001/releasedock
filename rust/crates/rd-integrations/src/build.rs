//! Port of `Release/BuildService.swift`. `BuildResult` lives in `rd-records`.
//!
//! The build command is the user's own configuration and runs with their permissions, on a
//! copy of the development folder, in the platform's shell:
//!
//! - **macOS:** a login `zsh` (`/bin/zsh -l -c <command>`), so the user's Homebrew/Node setup
//!   from `~/.zprofile` applies, exactly as in Swift. The copy is an APFS clone
//!   (`cp -c -R`, which uses `clonefile`), falling back to a regular copy.
//! - **Linux:** a login POSIX shell (`/bin/sh -l -c <command>`), which reads `/etc/profile`
//!   and `~/.profile`. The copy is a plain recursive copy.
//! - **Windows:** `cmd.exe` (`%ComSpec%`), because `&&`, `npm` and `composer` (`.cmd` shims)
//!   work there on every Windows version, while Windows PowerShell 5.1 has no `&&`. The
//!   command is handed over in the `RELEASEDOCK_BUILD_COMMAND` environment variable and run
//!   as `cmd /D /S /C %RELEASEDOCK_BUILD_COMMAND%`: Windows has no argument array, and
//!   cmd.exe does not understand the `\"` quoting Rust uses for arguments, so this is the
//!   only way to pass any command text through unchanged. `/D` skips AutoRun scripts.
//!   The copy is a plain recursive copy. A PowerShell executable set with
//!   [`BuildService::with_shell`] runs `-NoProfile -NonInteractive -Command <command>`.

use async_trait::async_trait;
use rd_records::BuildResult;
use rd_svn::process_runner::describe_duration;
use rd_svn::{ProcessRequest, ProcessResult, ProcessRunner, ProcessRunnerError, ProcessRunning};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

/// Errors of [`Building::build`]. `BuildFailed` is Swift's `ReleaseError.buildFailed` with
/// the same message; the release pipeline maps it to its own error type.
#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    #[error(
        "The build command failed (exit code {exit_code}): {command}\n{}",
        suffix(output, 800)
    )]
    BuildFailed {
        command: String,
        exit_code: i32,
        output: String,
    },
    /// The folder could not be copied (Swift rethrows the Foundation error).
    #[error("{0}")]
    Copy(String),
    /// The shell could not be started (Swift rethrows `ProcessRunnerError`).
    #[error(transparent)]
    Process(ProcessRunnerError),
}

#[async_trait]
pub trait Building: Send + Sync {
    /// Copies `source` into `workspace` and runs `command` there. Never modifies `source`.
    async fn build(&self, source: &Path, command: &str, workspace: &Path) -> Result<BuildResult, BuildError>;
}

/// Environment variable that carries the build command to `cmd.exe` on Windows.
pub const WINDOWS_COMMAND_VARIABLE: &str = "RELEASEDOCK_BUILD_COMMAND";

/// Runs a product's build command (e.g. `composer install --no-dev && npm run build`) in the
/// platform's login shell on a copy of the folder (see the module documentation).
#[derive(Clone)]
pub struct BuildService {
    runner: Arc<dyn ProcessRunning>,
    shell: PathBuf,
    timeout: Duration,
}

impl Default for BuildService {
    fn default() -> Self {
        Self::new()
    }
}

impl BuildService {
    pub const OUTPUT_TAIL_LENGTH: usize = 6000;

    /// A real process runner, the platform shell and a 30 minute timeout.
    pub fn new() -> Self {
        Self {
            runner: Arc::new(ProcessRunner::default()),
            shell: Self::default_shell(),
            timeout: Duration::from_secs(1800),
        }
    }

    pub fn with_runner(mut self, runner: Arc<dyn ProcessRunning>) -> Self {
        self.runner = runner;
        self
    }

    pub fn with_shell(mut self, shell: impl Into<PathBuf>) -> Self {
        self.shell = shell.into();
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// `/bin/zsh` on macOS, `/bin/sh` on other Unix systems, `%ComSpec%` (cmd.exe) on Windows.
    pub fn default_shell() -> PathBuf {
        #[cfg(target_os = "macos")]
        return PathBuf::from("/bin/zsh");
        #[cfg(all(unix, not(target_os = "macos")))]
        return PathBuf::from("/bin/sh");
        #[cfg(windows)]
        return std::env::var_os("ComSpec")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows\System32\cmd.exe"));
    }

    /// The process request that runs `command` with `shell` in `directory`.
    pub fn shell_request(shell: &Path, command: &str, directory: &Path, timeout: Duration) -> ProcessRequest {
        #[cfg(unix)]
        let request = ProcessRequest::new(shell, vec!["-l".into(), "-c".into(), command.into()]);
        #[cfg(windows)]
        let request = {
            let name = shell
                .file_stem()
                .map(|s| s.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            if name == "powershell" || name == "pwsh" {
                ProcessRequest::new(
                    shell,
                    vec![
                        "-NoProfile".into(),
                        "-NonInteractive".into(),
                        "-Command".into(),
                        command.into(),
                    ],
                )
            } else {
                let mut request = ProcessRequest::new(
                    shell,
                    vec![
                        "/D".into(),
                        "/S".into(),
                        "/C".into(),
                        format!("%{WINDOWS_COMMAND_VARIABLE}%"),
                    ],
                );
                request
                    .environment
                    .insert(WINDOWS_COMMAND_VARIABLE.into(), command.into());
                request
            }
        };
        request.working_directory(directory).timeout(timeout)
    }

    /// The end of the combined output, at most [`Self::OUTPUT_TAIL_LENGTH`] characters.
    pub fn tail(result: &ProcessResult) -> String {
        let combined = [result.stdout(), result.stderr()]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        suffix(&combined, Self::OUTPUT_TAIL_LENGTH).to_owned()
    }

    /// Replaces `target` with a copy of `source`: an APFS clone on macOS (copy-on-write,
    /// near instant), falling back to a regular recursive copy across volumes, on file
    /// systems without cloning, and on other platforms.
    pub fn clone_tree(source: &Path, target: &Path) -> io::Result<()> {
        if fs::symlink_metadata(target).is_ok() {
            remove_item(target)?;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        #[cfg(target_os = "macos")]
        {
            let cloned = std::process::Command::new("/bin/cp")
                .arg("-c")
                .arg("-R")
                .arg(source)
                .arg(target)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .is_ok_and(|status| status.success());
            if cloned {
                return Ok(());
            }
            if fs::symlink_metadata(target).is_ok() {
                remove_item(target)?;
            }
        }
        copy_item(source, target)
    }
}

#[async_trait]
impl Building for BuildService {
    async fn build(&self, source: &Path, command: &str, workspace: &Path) -> Result<BuildResult, BuildError> {
        let name = source.file_name().map(PathBuf::from).unwrap_or_default();
        let target = workspace.join("build").join(name);
        let (from, to) = (source.to_path_buf(), target.clone());
        tokio::task::spawn_blocking(move || Self::clone_tree(&from, &to))
            .await
            .map_err(|error| BuildError::Copy(error.to_string()))?
            .map_err(|error| BuildError::Copy(error.to_string()))?;

        let request = Self::shell_request(&self.shell, command, &target, self.timeout);
        let result = match self.runner.run(request).await {
            Ok(result) => result,
            Err(ProcessRunnerError::TimedOut(partial)) => {
                return Err(BuildError::BuildFailed {
                    command: command.to_owned(),
                    exit_code: partial.exit_code,
                    output: format!(
                        "{}\n(timed out after {})",
                        Self::tail(&partial),
                        describe_duration(partial.duration)
                    ),
                });
            }
            Err(error) => return Err(BuildError::Process(error)),
        };
        if !result.succeeded() {
            return Err(BuildError::BuildFailed {
                command: command.to_owned(),
                exit_code: result.exit_code,
                output: Self::tail(&result),
            });
        }
        Ok(BuildResult::new(
            command,
            result.exit_code,
            result.duration.as_secs_f64(),
            Self::tail(&result),
            target.to_string_lossy(),
        ))
    }
}

/// The last `count` characters of `text` (Swift `String.suffix`).
fn suffix(text: &str, count: usize) -> &str {
    let total = text.chars().count();
    if total <= count {
        return text;
    }
    let start = text.char_indices().nth(total - count).map_or(0, |(index, _)| index);
    &text[start..]
}

/// `FileManager.removeItem(at:)`: a file, a symbolic link (not its target) or a directory tree.
fn remove_item(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

/// `FileManager.copyItem(at:to:)`: copies a tree, keeping symbolic links as links.
fn copy_item(source: &Path, target: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(source)?;
    if metadata.file_type().is_symlink() {
        return copy_symlink(source, target);
    }
    if metadata.is_dir() {
        fs::create_dir(target)?;
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            copy_item(&entry.path(), &target.join(entry.file_name()))?;
        }
        fs::set_permissions(target, metadata.permissions())?;
        return Ok(());
    }
    fs::copy(source, target).map(|_| ())
}

#[cfg(unix)]
fn copy_symlink(source: &Path, target: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(fs::read_link(source)?, target)
}

#[cfg(windows)]
fn copy_symlink(source: &Path, target: &Path) -> io::Result<()> {
    let destination = fs::read_link(source)?;
    if fs::metadata(source).is_ok_and(|m| m.is_dir()) {
        std::os::windows::fs::symlink_dir(destination, target)
    } else {
        std::os::windows::fs::symlink_file(destination, target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suffix_counts_characters() {
        assert_eq!(suffix("abcdef", 3), "def");
        assert_eq!(suffix("ab", 3), "ab");
        assert_eq!(suffix("aé€😀", 2), "€😀");
    }

    #[test]
    fn tail_joins_non_empty_streams() {
        let result = ProcessResult {
            exit_code: 0,
            standard_output: b"out".to_vec(),
            standard_error: Vec::new(),
            duration: Duration::ZERO,
            display_command: String::new(),
        };
        assert_eq!(BuildService::tail(&result), "out");
        let both = ProcessResult {
            standard_error: b"err".to_vec(),
            ..result
        };
        assert_eq!(BuildService::tail(&both), "out\nerr");
    }
}
