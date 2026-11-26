//! Rust-only tests for `GitService` (the Swift tests only use `StubGit`). They run real
//! `git` in a temporary repository and skip themselves when git is not installed.

mod common;

use common::TempDir;
use rd_integrations::{GitService, GitServicing};
use std::path::Path;
use std::process::Command;

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new(GitService::new().executable().expect("git"))
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=Tests",
            "-c",
            "user.email=tests@example.com",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("run git");
    assert!(status.success(), "git {args:?}");
}

fn service() -> Option<GitService> {
    let service = GitService::new();
    if service.executable().is_none() {
        eprintln!("skipped: git not installed");
        return None;
    }
    Some(service)
}

#[tokio::test]
async fn reads_commit_branch_tag_changes_and_redacted_remote() {
    let Some(service) = service() else { return };
    if !service.is_available().await {
        eprintln!("skipped: git does not run");
        return;
    }
    let dir = TempDir::new();
    let repo = dir.path("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    dir.write("repo/a.txt", "a");
    git(&repo, &["add", "a.txt"]);
    git(&repo, &["commit", "-q", "-m", "First"]);
    git(&repo, &["tag", "v1.0.0"]);
    git(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            "https://user:ghp_secret123@github.com/acme/x.git",
        ],
    );

    let state = service.state(&repo).await.unwrap();
    assert!(state.is_repository);
    assert_eq!(state.head_commit.as_ref().map(String::len), Some(40));
    assert_eq!(state.branch.as_deref(), Some("main"));
    assert_eq!(state.exact_tag.as_deref(), Some("v1.0.0"));
    assert!(state.is_clean());
    let remote = state.remote_url.unwrap();
    assert!(!remote.contains("ghp_secret123"), "{remote}");

    dir.write("repo/a.txt", "changed");
    dir.write("repo/b.txt", "new");
    let state = service.state(&repo).await.unwrap();
    assert_eq!(
        state.uncommitted_changes,
        vec![" M a.txt".to_owned(), "?? b.txt".to_owned()]
    );
    assert_eq!(state.exact_tag.as_deref(), Some("v1.0.0"));
}

#[tokio::test]
async fn plain_folder_is_not_a_repository() {
    let Some(service) = service() else { return };
    let dir = TempDir::new();
    dir.mkdir("plain");
    let state = service.state(&dir.path("plain")).await.unwrap();
    assert_eq!(state, rd_integrations::GitRepositoryState::not_a_repository());
}

#[tokio::test]
async fn missing_git_is_unavailable() {
    let service = GitService::new().with_executable("/nonexistent/git");
    assert!(!service.is_available().await);
    assert!(service.state(Path::new("/tmp")).await.is_err());
}
