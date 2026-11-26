//! Ported from `BuildTests.swift` (`BuildServiceTests`; `ExportIgnoreTests` are in
//! `rd-inspection`, the pipeline tests are listed in the README). The commands are POSIX
//! shell, so these run on Unix; Windows has its own cmd.exe test.

mod common;

use common::TempDir;
use common::fixtures::make_plugin;
use rd_integrations::{BuildError, BuildService, Building};
use std::collections::BTreeMap;
#[cfg(unix)]
use std::path::Path;
use std::path::PathBuf;

#[cfg(unix)]
fn subpaths(root: &Path) -> Vec<String> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).expect("walk") {
            let path = entry.expect("walk").path();
            out.push(path.strip_prefix(root).expect("walk").to_string_lossy().into_owned());
            if path.is_dir() {
                walk(root, &path, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort();
    out
}

fn source(dir: &TempDir, extra: &BTreeMap<&str, &str>) -> PathBuf {
    make_plugin(dir, "dev/sample-plugin", "1.1.0", None, "Sample Plugin", extra)
}

#[cfg(unix)]
#[tokio::test]
async fn builds_on_a_clone_and_leaves_the_source_untouched() {
    let dir = TempDir::new();
    let source = source(&dir, &BTreeMap::from([("src/app.ts", "let a = 1")]));
    let before = subpaths(&source);
    // Swift uses `rm -rf src`; `rm -r` removes the same plain files without forcing.
    let result = BuildService::new()
        .build(
            &source,
            "mkdir -p build && echo compiled > build/app.js && rm -r src && echo done",
            &dir.path("staging"),
        )
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0);
    assert!(result.output_tail.contains("done"));
    assert!(
        Path::new(&result.built_path).ends_with("build/sample-plugin"),
        "keeps the folder name"
    );
    let built = PathBuf::from(&result.built_path);
    assert_eq!(
        std::fs::read_to_string(built.join("build/app.js")).unwrap(),
        "compiled\n"
    );
    assert!(!built.join("src").exists());
    assert_eq!(subpaths(&source), before, "source untouched");
}

#[cfg(unix)]
#[tokio::test]
async fn failing_build_reports_exit_code_and_output() {
    let dir = TempDir::new();
    let source = source(&dir, &BTreeMap::new());
    let error = BuildService::new()
        .build(
            &source,
            "echo 'npm ERR! missing script: build' >&2; exit 3",
            &dir.path("staging"),
        )
        .await
        .unwrap_err();
    let BuildError::BuildFailed {
        command,
        exit_code,
        output,
    } = error
    else {
        panic!("expected a build failure, got {error:?}");
    };
    assert_eq!(exit_code, 3);
    assert!(command.contains("exit 3"));
    assert!(output.contains("missing script: build"));
}

#[cfg(unix)]
#[tokio::test]
async fn rebuild_replaces_the_previous_copy() {
    let dir = TempDir::new();
    let source = source(&dir, &BTreeMap::new());
    let first = BuildService::new()
        .build(&source, "touch stale.txt", &dir.path("staging"))
        .await
        .unwrap();
    let second = BuildService::new()
        .build(&source, "true", &dir.path("staging"))
        .await
        .unwrap();
    assert_eq!(first.built_path, second.built_path);
    assert!(!Path::new(&second.built_path).join("stale.txt").exists());
}

// MARK: Rust-only

#[test]
fn build_failure_message_matches_swift() {
    let error = BuildError::BuildFailed {
        command: "npm run build".into(),
        exit_code: 1,
        output: format!("{}tail", "x".repeat(900)),
    };
    let message = error.to_string();
    assert!(message.starts_with("The build command failed (exit code 1): npm run build\n"));
    assert!(message.ends_with("tail"));
    assert_eq!(
        message.lines().nth(1).unwrap().chars().count(),
        800,
        "the last 800 characters"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn timeout_is_a_build_failure_with_the_partial_output() {
    let dir = TempDir::new();
    let source = source(&dir, &BTreeMap::new());
    let error = BuildService::new()
        .with_timeout(std::time::Duration::from_secs(2))
        .build(&source, "echo started; exec tail -f /dev/null", &dir.path("staging"))
        .await
        .unwrap_err();
    let BuildError::BuildFailed { output, .. } = error else {
        panic!("expected a build failure, got {error:?}");
    };
    assert!(output.contains("started"), "{output}");
    assert!(output.contains("(timed out after "), "{output}");
}

#[cfg(unix)]
#[test]
fn clone_keeps_symbolic_links() {
    let dir = TempDir::new();
    let source = source(&dir, &BTreeMap::new());
    std::os::unix::fs::symlink("readme.txt", source.join("link.txt")).unwrap();
    let target = dir.path("copy/sample-plugin");
    BuildService::clone_tree(&source, &target).unwrap();
    assert_eq!(
        std::fs::read_link(target.join("link.txt")).unwrap(),
        PathBuf::from("readme.txt")
    );
    assert_eq!(subpaths(&target), subpaths(&source));
}

#[cfg(windows)]
#[tokio::test]
async fn builds_with_cmd_on_windows() {
    let dir = TempDir::new();
    let source = source(&dir, &BTreeMap::new());
    let result = BuildService::new()
        .build(
            &source,
            r#"mkdir build && echo compiled> build\app.js && echo "done""#,
            &dir.path("staging"),
        )
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0);
    assert!(result.output_tail.contains("\"done\""), "{}", result.output_tail);
    let built = PathBuf::from(&result.built_path);
    assert!(built.ends_with(r"build\sample-plugin"));
    assert_eq!(
        std::fs::read_to_string(built.join(r"build\app.js")).unwrap().trim_end(),
        "compiled"
    );
    let error = BuildService::new()
        .build(&source, "exit /b 3", &dir.path("staging"))
        .await
        .unwrap_err();
    assert!(
        matches!(error, BuildError::BuildFailed { exit_code: 3, .. }),
        "{error:?}"
    );
}
