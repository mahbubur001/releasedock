//! Integration tests against a local `file://` repository created with svnadmin
//! (`LocalSVNRepository` in TestSupport.swift). Rust-only: in the Swift suite the
//! service is exercised through the pipeline tests, which are not ported yet. Every test
//! skips when svn or svnadmin is not installed.

mod support;

use rd_model::Id;
use rd_svn::*;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use support::{LocalSvnRepository, TempDir};

fn service() -> SvnService {
    SvnService::new().with_command_timeout(Duration::from_secs(60))
}

#[tokio::test]
async fn detects_the_installed_client() {
    let Some(_repo) = LocalSvnRepository::new_or_skip() else {
        return;
    };
    let installation = service().detect_installation().await.unwrap();
    assert!(installation.version.starts_with("1."), "{}", installation.version);
    assert!(installation.supports_password_from_stdin());
    assert_eq!(Some(installation.executable), LocalSvnRepository::svn_executable());
}

#[tokio::test]
async fn reads_a_repository_without_a_working_copy() {
    let Some(repo) = LocalSvnRepository::new_or_skip() else {
        return;
    };
    repo.create_plugin("sample-plugin", Some("1.0.0"), "Sample Plugin")
        .unwrap();
    let svn = service();
    let base = format!("{}/sample-plugin", repo.url());

    let info = svn.info(&base).await.unwrap();
    assert_eq!(info.kind, SvnNodeKind::Dir);
    assert_eq!(info.url, base);
    assert_eq!(info.repository_root, repo.url());
    assert_eq!(info.last_changed_revision, Some(2));
    assert!(info.repository_uuid.is_some());
    assert!(info.working_copy_root.is_none());

    assert!(svn.exists(&format!("{base}/tags/1.0.0")).await.unwrap());
    assert!(
        !svn.exists(&format!("{base}/tags/9.9.9")).await.unwrap(),
        "missing is false, not an error"
    );

    let tags = svn.list(&format!("{base}/tags"), false).await.unwrap();
    assert_eq!(
        tags.iter().map(|t| (t.name.as_str(), t.kind)).collect::<Vec<_>>(),
        [("1.0.0", SvnNodeKind::Dir)]
    );
    let files = svn.list(&format!("{base}/trunk"), true).await.unwrap();
    let readme = files.iter().find(|f| f.name == "readme.txt").unwrap();
    assert_eq!(readme.kind, SvnNodeKind::File);
    assert!(readme.size.unwrap() > 100);
    assert!(files.iter().any(|f| f.name == "includes/class-sample.php"));

    let readme = String::from_utf8(svn.cat(&format!("{base}/trunk/readme.txt")).await.unwrap()).unwrap();
    assert!(readme.contains("Stable tag: 1.0.0"));

    let log = svn.log(&base, 10).await.unwrap();
    assert_eq!(log.iter().map(|e| e.revision).collect::<Vec<_>>(), [2, 1]);
    assert_eq!(log[0].message, "Initial 1.0.0");
    assert!(
        log[0].changed_paths.contains(&"/sample-plugin/tags/1.0.0".to_owned()),
        "{:?}",
        log[0].changed_paths
    );

    match svn.cat(&format!("{base}/trunk/missing.txt")).await {
        Err(error) => assert_eq!(error.kind(), Some(SvnErrorKind::NotFound), "{error}"),
        Ok(_) => panic!("expected notFound"),
    }
}

#[tokio::test]
async fn working_copy_round_trip() {
    let Some(repo) = LocalSvnRepository::new_or_skip() else {
        return;
    };
    repo.create_plugin("sample-plugin", Some("1.0.0"), "Sample Plugin")
        .unwrap();
    let work = TempDir::new();
    let svn = service();
    let base = format!("{}/sample-plugin", repo.url());
    let wc = work.path("wc");

    svn.checkout(&base, &wc, SvnDepth::Infinity).await.unwrap();
    let info = svn.info(&wc.to_string_lossy()).await.unwrap();
    assert_eq!(info.revision, 2);
    assert_eq!(info.working_copy_root.as_deref(), Some(&*wc.to_string_lossy()));
    assert!(svn.status(&wc, false).await.unwrap().is_empty());

    // Change, add (including a peg-revision-like name and a globally ignored name), delete.
    let trunk = wc.join("trunk");
    std::fs::write(trunk.join("readme.txt"), "changed\n").unwrap();
    std::fs::create_dir_all(trunk.join("assets/img")).unwrap();
    std::fs::write(trunk.join("assets/img/logo@2x.png"), "PNG").unwrap();
    std::fs::write(trunk.join("lib.so"), "binary").unwrap();
    let added: Vec<PathBuf> = vec![trunk.join("assets/img/logo@2x.png"), trunk.join("lib.so")];
    svn.add(&added).await.unwrap();
    svn.delete(&[trunk.join("includes/class-sample.php")]).await.unwrap();
    svn.set_property("svn:mime-type", "image/png", &[trunk.join("assets/img/logo@2x.png")])
        .await
        .unwrap();

    let status = svn.status(&wc, false).await.unwrap();
    let item = |suffix: &str| status.iter().find(|e| e.path.ends_with(suffix)).map(|e| e.item);
    assert_eq!(item("trunk/readme.txt"), Some(SvnItemStatus::Modified));
    assert_eq!(item("assets/img/logo@2x.png"), Some(SvnItemStatus::Added));
    assert_eq!(
        item("assets/img"),
        Some(SvnItemStatus::Added),
        "--parents adds intermediate folders"
    );
    assert_eq!(
        item("trunk/lib.so"),
        Some(SvnItemStatus::Added),
        "--no-ignore keeps global-ignores files"
    );
    assert_eq!(item("includes/class-sample.php"), Some(SvnItemStatus::Deleted));
    assert!(status.iter().all(SvnStatusEntry::is_local_change));
    assert!(svn.diff(&wc).await.unwrap().contains("+changed"));

    svn.copy(&trunk, &wc.join("tags/1.1.0")).await.unwrap();
    let revision = svn
        .commit(std::slice::from_ref(&wc), "Release 1.1.0\n\nWith ünïcode", None)
        .await
        .unwrap();
    assert_eq!(revision, 3);
    assert!(
        svn.exists(&format!("{base}/tags/1.1.0/assets/img/logo@2x.png"))
            .await
            .unwrap()
    );
    let log = svn.log(&base, 1).await.unwrap();
    assert_eq!(log[0].message, "Release 1.1.0\n\nWith ünïcode");

    // Nothing left to commit.
    match svn.commit(std::slice::from_ref(&wc), "again", None).await {
        Err(SvnError::UnexpectedOutput(message)) => assert_eq!(message, "svn commit reported nothing to commit"),
        other => panic!("expected unexpectedOutput, got {other:?}"),
    }

    // A second working copy is out of date until updated.
    let other = work.path("other");
    svn.checkout(&base, &other, SvnDepth::Immediates).await.unwrap();
    assert!(
        !other.join("trunk/readme.txt").exists(),
        "immediates leaves folders empty"
    );
    assert_eq!(svn.update(&other, Some(SvnDepth::Infinity)).await.unwrap(), 3);
    assert_eq!(
        std::fs::read_to_string(other.join("trunk/readme.txt")).unwrap(),
        "changed\n"
    );

    std::fs::write(other.join("trunk/readme.txt"), "local edit\n").unwrap();
    svn.revert(&other).await.unwrap();
    assert!(svn.status(&other, false).await.unwrap().is_empty());
    svn.cleanup(&other).await.unwrap();

    let export = work.path("export");
    svn.export(&format!("{base}/tags/1.1.0"), &export).await.unwrap();
    assert!(export.join("assets/img/logo@2x.png").exists());
    assert!(!export.join(".svn").exists());
}

#[tokio::test]
async fn out_of_date_commit_and_conflict_resolution() {
    let Some(repo) = LocalSvnRepository::new_or_skip() else {
        return;
    };
    repo.create_plugin("sample-plugin", Some("1.0.0"), "Sample Plugin")
        .unwrap();
    let work = TempDir::new();
    let svn = service();
    let base = format!("{}/sample-plugin", repo.url());
    let (first, second) = (work.path("first"), work.path("second"));
    svn.checkout(&base, &first, SvnDepth::Infinity).await.unwrap();
    svn.checkout(&base, &second, SvnDepth::Infinity).await.unwrap();

    std::fs::write(first.join("trunk/readme.txt"), "first\n").unwrap();
    svn.commit(std::slice::from_ref(&first), "first", None).await.unwrap();

    std::fs::write(second.join("trunk/readme.txt"), "second\n").unwrap();
    let status = svn.status(&second, true).await.unwrap();
    let readme = status.iter().find(|e| e.path.ends_with("trunk/readme.txt")).unwrap();
    assert!(readme.is_out_of_date());
    assert!(readme.is_local_change());
    match svn.commit(std::slice::from_ref(&second), "second", None).await {
        Err(error) => assert_eq!(error.kind(), Some(SvnErrorKind::OutOfDate), "{error}"),
        Ok(revision) => panic!("expected outOfDate, committed r{revision}"),
    }

    // svn update with --non-interactive postpones the conflict.
    svn.update(&second, None).await.unwrap();
    let conflicted: Vec<_> = svn
        .status(&second, false)
        .await
        .unwrap()
        .into_iter()
        .filter(|e| e.is_conflict())
        .collect();
    assert_eq!(conflicted.len(), 1);
    svn.resolve(&[second.join("trunk/readme.txt")], SvnConflictChoice::MineFull)
        .await
        .unwrap();
    assert!(
        !svn.status(&second, false)
            .await
            .unwrap()
            .iter()
            .any(SvnStatusEntry::is_conflict)
    );
    assert_eq!(
        std::fs::read_to_string(second.join("trunk/readme.txt")).unwrap(),
        "second\n"
    );
    assert_eq!(
        svn.commit(std::slice::from_ref(&second), "second", None).await.unwrap(),
        4
    );
}

#[tokio::test]
async fn errors_are_classified() {
    let Some(repo) = LocalSvnRepository::new_or_skip() else {
        return;
    };
    let work = TempDir::new();
    work.mkdir("plain");
    let svn = service();
    let plain = work.path("plain");
    match svn.info(&plain.to_string_lossy()).await {
        Err(error) => assert_eq!(error.kind(), Some(SvnErrorKind::NotAWorkingCopy), "{error}"),
        Ok(info) => panic!("expected notAWorkingCopy, got {info:?}"),
    }
    // svn itself only warns (W155007, exit 0) for `status` outside a working copy.
    assert!(svn.status(&plain, false).await.unwrap().is_empty());
    match svn.info(&format!("{}/missing", repo.url())).await {
        Err(SvnError::CommandFailed {
            kind, codes, command, ..
        }) => {
            assert_eq!(kind, SvnErrorKind::NotFound);
            assert!(codes.iter().any(|c| c == "E200009" || c == "W170000"), "{codes:?}");
            assert!(command.contains("info --xml"), "{command}");
        }
        other => panic!("expected commandFailed, got {other:?}"),
    }
}

#[tokio::test]
async fn transcript_records_real_commands_in_scope() {
    let Some(repo) = LocalSvnRepository::new_or_skip() else {
        return;
    };
    repo.create_theme("sample-theme", Some("1.2.0"), "Sample Theme")
        .unwrap();
    let work = TempDir::new();
    let transcript = Arc::new(SvnTranscript::new(work.path("Transcripts")));
    let svn = service().with_transcript(transcript.clone());
    let product = Id::new();
    let base = format!("{}/sample-theme", repo.url());

    SvnTranscript::with_scope(TranscriptScope::new(product, None, "Prepare"), async {
        svn.list(&base, false).await.unwrap();
        assert!(!svn.exists(&format!("{base}/9.9.9")).await.unwrap());
        svn.cat(&format!("{base}/1.2.0/style.css")).await.unwrap();
    })
    .await;
    svn.info(&base).await.unwrap(); // outside the scope

    let entries = transcript.entries(product, None, 100);
    let commands: Vec<&str> = entries
        .iter()
        .map(|e| e.command.split(' ').nth(1).unwrap_or(""))
        .collect();
    assert_eq!(commands, ["list", "info", "cat"]);
    assert!(entries[0].succeeded());
    assert!(
        entries[0].output.starts_with('(') && entries[0].output.ends_with(" of output)"),
        "XML is summarised"
    );
    assert_eq!(entries[1].exit_code, Some(1));
    assert!(entries[1].output.contains("E200009"), "{}", entries[1].output);
    assert!(
        entries[2].output.ends_with("of output)"),
        "cat output is data: {}",
        entries[2].output
    );
}

#[tokio::test]
async fn cancelling_stops_a_running_svn_command() {
    let Some(repo) = LocalSvnRepository::new_or_skip() else {
        return;
    };
    repo.create_plugin("sample-plugin", Some("1.0.0"), "Sample Plugin")
        .unwrap();
    let svn = service();
    svn.detect_installation().await.unwrap();
    let token = CancellationToken::new();
    token.cancel();
    let outcome = with_cancellation(token, svn.info(&format!("{}/sample-plugin", repo.url()))).await;
    match outcome {
        Err(SvnError::Process(ProcessRunnerError::Cancelled(partial))) => {
            assert!(partial.display_command.contains("info --xml"))
        }
        // svn may finish before the signal lands; then the result is simply returned.
        Ok(info) => assert_eq!(info.kind, SvnNodeKind::Dir),
        Err(other) => panic!("expected cancellation, got {other:?}"),
    }
}
