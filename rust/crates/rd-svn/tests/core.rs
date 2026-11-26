//! Ported from CoreTests.swift: ProcessRunnerTests, SVNParsingTests and SVNTimeoutTests
//! (with RecordingRunner in `support`).

mod support;

use rd_svn::*;
use std::sync::Arc;
use std::time::Duration;
use support::{RecordingRunner, TempDir, stand_in_executable};

// MARK: ProcessRunnerTests

#[cfg(unix)]
#[tokio::test]
async fn arguments_are_not_interpreted_by_a_shell() {
    let dir = TempDir::new();
    let marker = dir.path("pwned");
    let marker_text = marker.to_string_lossy().into_owned();
    let arguments = vec![
        format!("$(touch {marker_text})"),
        "; rm -rf /".to_owned(),
        "`id`".to_owned(),
    ];
    let result = ProcessRunner::default()
        .run(ProcessRequest::new("/bin/echo", arguments))
        .await
        .unwrap();
    assert_eq!(result.stdout(), format!("$(touch {marker_text}) ; rm -rf / `id`\n"));
    assert!(!marker.exists());
}

#[cfg(unix)]
#[tokio::test]
async fn stdin_and_capture() {
    let request = ProcessRequest::new("/bin/cat", vec![]).standard_input(b"secret\n".to_vec());
    let result = ProcessRunner::default().run(request).await.unwrap();
    assert_eq!(result.stdout(), "secret\n");
    assert_eq!(result.exit_code, 0);
    assert!(result.duration > Duration::ZERO);
}

#[cfg(unix)]
#[tokio::test]
async fn timeout() {
    let request = ProcessRequest::new("/bin/sleep", vec!["10".into()]).timeout(Duration::from_millis(300));
    match ProcessRunner::new(Duration::from_secs(1)).run(request).await {
        Err(ProcessRunnerError::TimedOut(partial)) => assert!(partial.duration < Duration::from_secs(5)),
        other => panic!("expected timeout, got {other:?}"),
    }
}

#[cfg(unix)]
#[tokio::test]
async fn timeout_preserves_partial_output() {
    // A process that has written something before it is stopped: the output is kept.
    let script = "echo 'Committed revision 7.'; exec sleep 10";
    let request = ProcessRequest::new("/bin/sh", vec!["-c".into(), script.into()]).timeout(Duration::from_millis(500));
    match ProcessRunner::new(Duration::from_secs(1)).run(request).await {
        Err(ProcessRunnerError::TimedOut(partial)) => assert_eq!(partial.stdout(), "Committed revision 7.\n"),
        other => panic!("expected timeout, got {other:?}"),
    }
}

#[cfg(unix)]
#[tokio::test]
async fn cancellation() {
    let token = CancellationToken::new();
    let task = tokio::spawn(with_cancellation(token.clone(), async {
        ProcessRunner::default()
            .run(ProcessRequest::new("/bin/sleep", vec!["10".into()]))
            .await
    }));
    tokio::time::sleep(Duration::from_millis(200)).await;
    token.cancel();
    match task.await.unwrap() {
        Err(ProcessRunnerError::Cancelled(_)) => {}
        other => panic!("expected cancellation, got {other:?}"),
    }
}

#[test]
fn redacted_display() {
    let request = ProcessRequest::new("/bin/x", vec!["--password".into(), "pw".into()]).redacted_argument_indices([1]);
    assert_eq!(request.display_command(), "/bin/x --password <redacted>");
}

#[test]
fn debug_output_never_shows_stdin() {
    let request = ProcessRequest::new("/bin/x", vec![]).standard_input(b"hunter2\n".to_vec());
    assert!(!format!("{request:?}").contains("hunter2"));
}

#[tokio::test]
async fn missing_executable_is_reported() {
    let missing = std::env::temp_dir().join(format!("no-such-tool-{}", rd_model::Id::new()));
    match ProcessRunner::default()
        .run(ProcessRequest::new(&missing, vec![]))
        .await
    {
        Err(ProcessRunnerError::ExecutableNotFound(path)) => assert_eq!(path, missing.to_string_lossy()),
        other => panic!("expected executableNotFound, got {other:?}"),
    }
}

#[test]
fn timed_out_message_reads_like_swift() {
    let result = ProcessResult {
        exit_code: 15,
        standard_output: vec![],
        standard_error: vec![],
        duration: Duration::from_millis(301),
        display_command: "/bin/sleep 10".into(),
    };
    assert_eq!(
        ProcessRunnerError::TimedOut(result.clone()).to_string(),
        "Timed out after 0.301 seconds: /bin/sleep 10"
    );
    assert_eq!(
        ProcessRunnerError::Cancelled(result).to_string(),
        "Cancelled: /bin/sleep 10"
    );
}

// MARK: SVNParsingTests

#[test]
fn peg_revision_escaping() {
    assert_eq!(SvnService::target("/wc/trunk/logo@2x.png"), "/wc/trunk/logo@2x.png@");
    assert_eq!(SvnService::target("/wc/trunk/a.php"), "/wc/trunk/a.php");
}

#[test]
fn error_classification() {
    fn kind(stderr: &str) -> SvnErrorKind {
        SvnErrorKind::classify(&SvnService::error_codes(stderr), stderr)
    }
    assert_eq!(
        kind("svn: E170001: Authorization failed"),
        SvnErrorKind::AuthenticationFailed
    );
    assert_eq!(
        kind("svn: E215004: No more credentials or we tried too many times."),
        SvnErrorKind::AuthenticationRequired
    );
    assert_eq!(
        kind("svn: E170013: Unable to connect to a repository at URL\nsvn: E670008: nodename nor servname provided"),
        SvnErrorKind::Network
    );
    assert_eq!(
        kind("svn: E165001: Commit blocked by pre-commit hook (exit code 1) with output:"),
        SvnErrorKind::HookRejected
    );
    assert_eq!(kind("svn: E155011: File '/x' is out of date"), SvnErrorKind::OutOfDate);
    assert_eq!(
        kind("svn: warning: W170000: URL 'x' non-existent in revision 5\nsvn: E200009: Could not display info"),
        SvnErrorKind::NotFound
    );
    assert_eq!(
        kind("svn: E230001: Server SSL certificate verification failed"),
        SvnErrorKind::Certificate
    );
    assert_eq!(
        kind("svn: E155004: Working copy locked"),
        SvnErrorKind::WorkingCopyLocked
    );
}

#[test]
fn committed_revision_parsing() {
    assert_eq!(
        SvnService::committed_revision(
            "Sending  a\nTransmitting file data .done\nCommitting transaction...\nCommitted revision 3816107.\n"
        ),
        Some(3816107)
    );
    assert_eq!(SvnService::committed_revision("Sending a\n"), None);
}

#[test]
fn status_xml_parsing() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<status><target path="/wc">
<entry path="/wc/trunk/a.php"><wc-status item="modified" props="none" revision="5"></wc-status></entry>
<entry path="/wc/tags/1.0"><wc-status item="added" props="none" copied="true"></wc-status></entry>
<entry path="/wc/trunk/b.php"><wc-status item="normal" props="none" revision="5"></wc-status><repos-status item="modified" props="none"></repos-status></entry>
</target></status>"#;
    let entries = SvnService::parse_status(&XmlElement::parse(xml.as_bytes()).unwrap());
    assert_eq!(entries.len(), 3);
    assert!(entries[0].is_local_change());
    assert!(entries[1].copied);
    assert!(entries[2].is_out_of_date());
    assert!(!entries[2].is_local_change());
}

// Rust-only: the Swift suite covers info and log parsing only through the integration tests.

#[test]
fn info_xml_parsing() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<info>
<entry kind="dir" path="trunk" revision="3816107">
<url>https://plugins.svn.wordpress.org/tlp-team/trunk</url>
<relative-url>^/tlp-team/trunk</relative-url>
<repository>
<root>https://plugins.svn.wordpress.org</root>
<uuid>b8457f37-d9ea-0310-8a92-e5e31aec5664</uuid>
</repository>
<wc-info><wcroot-abspath>/Users/me/svn/tlp-team</wcroot-abspath></wc-info>
<commit revision="3816100">
<author>techlabpro1</author>
<date>2026-11-20T10:11:12.345678Z</date>
</commit>
</entry>
</info>"#;
    let root = XmlElement::parse(xml.as_bytes()).unwrap();
    let info = SvnService::parse_info(root.descendants("entry")[0]);
    assert_eq!(
        info,
        SvnInfo {
            url: "https://plugins.svn.wordpress.org/tlp-team/trunk".into(),
            repository_root: "https://plugins.svn.wordpress.org".into(),
            repository_uuid: Some("b8457f37-d9ea-0310-8a92-e5e31aec5664".into()),
            revision: 3816107,
            kind: SvnNodeKind::Dir,
            last_changed_revision: Some(3816100),
            last_changed_author: Some("techlabpro1".into()),
            last_changed_date: Some("2026-11-20T10:11:12.345678Z".into()),
            working_copy_root: Some("/Users/me/svn/tlp-team".into()),
        }
    );
    let bare = XmlElement::parse(br#"<info><entry kind="weird" revision="x"/></info>"#).unwrap();
    let info = SvnService::parse_info(bare.descendants("entry")[0]);
    assert_eq!(
        (info.revision, info.kind, info.url.as_str()),
        (0, SvnNodeKind::Unknown, "")
    );
}

#[test]
fn log_and_list_xml_parsing() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<log>
<logentry revision="12">
<author>alice</author>
<date>2026-11-20T10:11:12.000000Z</date>
<paths>
<path action="M" kind="file">/sample/trunk/readme.txt</path>
<path action="A" kind="dir" copyfrom-path="/sample/trunk" copyfrom-rev="11">/sample/tags/1.2.0</path>
</paths>
<msg>  Release 1.2.0 &amp; fixes
</msg>
</logentry>
<logentry revision="11"><msg></msg></logentry>
</log>"#;
    let entries = SvnService::parse_log(&XmlElement::parse(xml.as_bytes()).unwrap());
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].revision, 12);
    assert_eq!(entries[0].author.as_deref(), Some("alice"));
    assert_eq!(
        entries[0].message, "  Release 1.2.0 & fixes\n",
        "the message is kept untrimmed"
    );
    assert_eq!(
        entries[0].changed_paths,
        vec!["/sample/trunk/readme.txt", "/sample/tags/1.2.0"]
    );
    assert_eq!((entries[1].author.clone(), entries[1].message.as_str()), (None, ""));

    let list = r#"<?xml version="1.0" encoding="UTF-8"?>
<lists><list path="https://x/tags">
<entry kind="dir"><name>1.0.0</name><commit revision="5"><author>a</author></commit></entry>
<entry kind="file"><name>logo@2x.png</name><size>2048</size><commit revision="6"/></entry>
</list></lists>"#;
    let entries = SvnService::parse_list(&XmlElement::parse(list.as_bytes()).unwrap());
    assert_eq!(
        entries,
        vec![
            SvnListEntry {
                name: "1.0.0".into(),
                kind: SvnNodeKind::Dir,
                size: None,
                commit_revision: Some(5)
            },
            SvnListEntry {
                name: "logo@2x.png".into(),
                kind: SvnNodeKind::File,
                size: Some(2048),
                commit_revision: Some(6)
            },
        ]
    );
}

#[test]
fn malformed_xml_is_rejected() {
    assert!(XmlElement::parse(b"<status><entry></status>").is_err());
    assert!(XmlElement::parse(b"").is_err());
}

// MARK: SVNTimeoutTests

#[tokio::test]
async fn commits_use_the_long_http_timeout_and_reads_the_short_one() {
    let runner = Arc::new(RecordingRunner::default());
    let svn = SvnService::new()
        .with_runner(runner.clone())
        .with_executable(stand_in_executable())
        .with_commit_timeout(Duration::from_secs(3600))
        .with_http_timeout_seconds(120)
        .with_commit_http_timeout_seconds(3600);
    let revision = svn.commit(&["/tmp/wc".into()], "Release", None).await.unwrap();
    assert_eq!(revision, 42);
    let _ = svn.cat("https://plugins.svn.wordpress.org/x/trunk/readme.txt").await;
    let requests = runner.requests();
    let commit = requests
        .iter()
        .find(|r| r.arguments.first().map(String::as_str) == Some("commit"))
        .unwrap();
    assert!(
        commit.arguments.iter().any(|a| a == "servers:global:http-timeout=3600"),
        "{:?}",
        commit.arguments
    );
    assert_eq!(commit.timeout, Duration::from_secs(3600));
    let cat = requests
        .iter()
        .find(|r| r.arguments.first().map(String::as_str) == Some("cat"))
        .unwrap();
    assert!(cat.arguments.iter().any(|a| a == "servers:global:http-timeout=120"));
}

// Rust-only: credentials go through stdin, never argv.
#[tokio::test]
async fn passwords_go_through_stdin_only() {
    let runner = Arc::new(RecordingRunner::default());
    let svn = SvnService::new()
        .with_runner(runner.clone())
        .with_executable(stand_in_executable());
    let credentials = SvnCredentials::new("alice", "s3cret-pass");
    svn.commit(&["/tmp/wc".into()], "Release", Some(&credentials))
        .await
        .unwrap();
    let requests = runner.requests();
    let commit = requests
        .iter()
        .find(|r| r.arguments.first().map(String::as_str) == Some("commit"))
        .unwrap();
    assert!(!commit.arguments.iter().any(|a| a.contains("s3cret-pass")));
    let tail: Vec<&str> = commit
        .arguments
        .iter()
        .rev()
        .take(4)
        .rev()
        .map(String::as_str)
        .collect();
    assert_eq!(
        tail,
        ["--username", "alice", "--password-from-stdin", "--no-auth-cache"]
    );
    assert_eq!(commit.standard_input.as_deref(), Some(&b"s3cret-pass\n"[..]));
    assert!(
        commit
            .arguments
            .starts_with(&["commit".to_owned(), "--file".to_owned()])
    );
}

#[test]
fn not_installed_message() {
    #[cfg(target_os = "macos")]
    assert_eq!(
        SvnError::NotInstalled.to_string(),
        "Subversion (svn) is not installed. Install it with Homebrew: brew install subversion"
    );
    assert!(
        SvnError::NotInstalled
            .to_string()
            .starts_with("Subversion (svn) is not installed.")
    );
    let failed = SvnError::CommandFailed {
        kind: SvnErrorKind::HookRejected,
        codes: vec!["E165001".into()],
        message: "blocked".into(),
        command: "svn commit".into(),
    };
    assert_eq!(failed.to_string(), "The repository rejected the commit: blocked");
    assert_eq!(failed.kind(), Some(SvnErrorKind::HookRejected));
    assert_eq!(
        SvnError::CommitOutcomeUncertain("interrupted".into()).to_string(),
        "The commit did not finish normally (interrupted). Its outcome is unknown; ReleaseDock will inspect the repository before doing anything else."
    );
}
