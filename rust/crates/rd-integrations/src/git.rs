//! Port of `Git/GitService.swift`. `GitRepositoryState` lives in `rd-records`.

use crate::discovery::{GIT_CANDIDATES, find_executable};
use async_trait::async_trait;
use rd_records::GitRepositoryState;
use rd_svn::{ProcessRequest, ProcessResult, ProcessRunner, ProcessRunnerError, ProcessRunning};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

#[async_trait]
pub trait GitServicing: Send + Sync {
    async fn is_available(&self) -> bool;
    /// Read-only inspection of a local development folder.
    async fn state(&self, directory: &Path) -> Result<GitRepositoryState, ProcessRunnerError>;
}

/// Read-only Git inspection used to record the source commit of a release and to
/// optionally require a clean development tree. ReleaseDock never writes to Git.
#[derive(Clone)]
pub struct GitService {
    runner: Arc<dyn ProcessRunning>,
    executable: Option<PathBuf>,
}

impl Default for GitService {
    fn default() -> Self {
        Self::new()
    }
}

impl GitService {
    /// A real process runner and the first `git` found (see [`GIT_CANDIDATES`]).
    pub fn new() -> Self {
        Self {
            runner: Arc::new(ProcessRunner::default()),
            executable: find_executable(GIT_CANDIDATES, "git"),
        }
    }

    pub fn with_runner(mut self, runner: Arc<dyn ProcessRunning>) -> Self {
        self.runner = runner;
        self
    }

    pub fn with_executable(mut self, executable: impl Into<PathBuf>) -> Self {
        self.executable = Some(executable.into());
        self
    }

    pub fn executable(&self) -> Option<&Path> {
        self.executable.as_deref()
    }

    async fn git(&self, args: &[&str], directory: &Path) -> Result<ProcessResult, ProcessRunnerError> {
        let Some(executable) = &self.executable else {
            return Err(ProcessRunnerError::ExecutableNotFound("git".into()));
        };
        let mut arguments = vec!["-C".to_owned(), directory.to_string_lossy().into_owned()];
        arguments.extend(args.iter().map(|a| (*a).to_owned()));
        let mut request = ProcessRequest::new(executable, arguments).timeout(Duration::from_secs(60));
        request.environment.insert("GIT_TERMINAL_PROMPT".into(), "0".into());
        // Never take index.lock during read-only status.
        request.environment.insert("GIT_OPTIONAL_LOCKS".into(), "0".into());
        self.runner.run(request).await
    }

    async fn value(&self, args: &[&str], directory: &Path) -> Result<Option<String>, ProcessRunnerError> {
        let result = self.git(args, directory).await?;
        if !result.succeeded() {
            return Ok(None);
        }
        let text = result.stdout().trim().to_owned();
        Ok((!text.is_empty()).then_some(text))
    }
}

#[async_trait]
impl GitServicing for GitService {
    async fn is_available(&self) -> bool {
        let Some(executable) = &self.executable else {
            return false;
        };
        // /usr/bin/git is a shim that fails when the Command Line Tools are missing.
        let request = ProcessRequest::new(executable, vec!["--version".into()]).timeout(Duration::from_secs(20));
        self.runner.run(request).await.is_ok_and(|result| result.succeeded())
    }

    async fn state(&self, directory: &Path) -> Result<GitRepositoryState, ProcessRunnerError> {
        let inside = self.git(&["rev-parse", "--is-inside-work-tree"], directory).await?;
        if !inside.succeeded() || inside.stdout().trim() != "true" {
            return Ok(GitRepositoryState::not_a_repository());
        }
        let status = self
            .git(&["status", "--porcelain=v1", "--untracked-files=normal"], directory)
            .await?;
        Ok(GitRepositoryState {
            is_repository: true,
            head_commit: self.value(&["rev-parse", "HEAD"], directory).await?,
            branch: self.value(&["rev-parse", "--abbrev-ref", "HEAD"], directory).await?,
            exact_tag: self
                .value(&["describe", "--tags", "--exact-match", "HEAD"], directory)
                .await?,
            uncommitted_changes: status
                .stdout()
                .split('\n')
                .filter(|line| !line.is_empty())
                .map(str::to_owned)
                .collect(),
            remote_url: self
                .value(&["config", "--get", "remote.origin.url"], directory)
                .await?
                .map(|url| rd_support::redact::redact(&url)),
        })
    }
}
