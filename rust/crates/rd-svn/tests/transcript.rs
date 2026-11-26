//! Ported from TranscriptTests.swift.

mod support;

use rd_model::{Id, SwiftDate};
use rd_svn::process_runner::home_directory;
use rd_svn::*;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use support::{RecordingRunner, TempDir, stand_in_executable};

fn service(transcript: Arc<SvnTranscript>) -> SvnService {
    SvnService::new()
        .with_runner(Arc::new(RecordingRunner::default()))
        .with_executable(stand_in_executable())
        .with_transcript(transcript)
}

fn result(exit_code: i32, stdout: &str, stderr: &str) -> ProcessResult {
    ProcessResult {
        exit_code,
        standard_output: stdout.as_bytes().to_vec(),
        standard_error: stderr.as_bytes().to_vec(),
        duration: Duration::ZERO,
        display_command: String::new(),
    }
}

#[tokio::test]
async fn records_scoped_commands_without_the_password() {
    let dir = TempDir::new();
    let folder = dir.path("transcripts");
    let transcript = Arc::new(SvnTranscript::new(&folder));
    let svn = service(transcript.clone());
    let (product, release) = (Id::new(), Id::new());
    let wc = PathBuf::from(home_directory()).join("svn/my plugin");
    let scope = TranscriptScope::new(product, Some(release), "Publish");
    SvnTranscript::with_scope(scope, async {
        let credentials = SvnCredentials::new("alice", "s3cret-pass");
        svn.commit(std::slice::from_ref(&wc), "Release 1.2.0", Some(&credentials))
            .await
            .unwrap();
    })
    .await;

    let entries = transcript.entries(product, Some(release), 10);
    assert_eq!(entries.len(), 1);
    let entry = &entries[0];
    assert_eq!(entry.operation, "Publish");
    assert!(entry.succeeded());
    assert!(entry.command.starts_with("svn commit"), "{}", entry.command);
    #[cfg(not(windows))]
    assert!(
        entry.command.contains("\"~/svn/my plugin\""),
        "home folder shortened and quoted: {}",
        entry.command
    );
    assert!(
        entry.command.contains("--username alice --password-from-stdin"),
        "{}",
        entry.command
    );
    assert!(
        !entry.command.contains("--non-interactive"),
        "global options are left out"
    );
    assert_eq!(entry.output, "Committed revision 42.");
    let file = std::fs::read_to_string(folder.join(format!("{product}.jsonl"))).unwrap();
    assert!(
        !file.contains("s3cret-pass"),
        "the password never reaches the transcript"
    );
}

#[tokio::test]
async fn unscoped_commands_are_not_recorded() {
    let dir = TempDir::new();
    let folder = dir.path("transcripts");
    let transcript = Arc::new(SvnTranscript::new(&folder));
    service(transcript)
        .commit(&["/tmp/wc".into()], "x", None)
        .await
        .unwrap();
    assert!(!folder.exists());
}

#[test]
fn data_output_is_summarised_and_long_output_trimmed() {
    let scope = TranscriptScope::new(Id::new(), None, "Prepare");
    let xml = result(0, &"<entry/>".repeat(200), "");
    let arguments = support::strings(&["status", "--xml", "/wc"]);
    let entry = SvnTranscript::entry(&scope, &arguments, None, Some(&xml), None, 0.2, SwiftDate::now());
    assert_eq!(entry.output, "(2 KB of output)");

    let lines: String = (1..=30).map(|i| format!("A    trunk/file{i}.php\n")).collect();
    let update = result(1, &lines, "svn: E170013: Unable to connect\n");
    let arguments = support::strings(&["update", "/wc"]);
    let entry = SvnTranscript::entry(&scope, &arguments, None, Some(&update), None, 1.0, SwiftDate::now());
    assert!(!entry.succeeded());
    assert!(
        entry.output.starts_with("… 18 more lines\nA    trunk/file19.php"),
        "{}",
        entry.output
    );
    assert!(
        entry.output.ends_with("svn: E170013: Unable to connect"),
        "{}",
        entry.output
    );
}

#[test]
fn keeps_the_newest_entries_and_filters_by_release() {
    let dir = TempDir::new();
    let transcript = SvnTranscript::with_maximum_entries(dir.path("transcripts"), 10);
    let (product, release) = (Id::new(), Id::new());
    for index in 0..400 {
        let scope = TranscriptScope::new(
            product,
            if index % 2 == 0 { Some(release) } else { None },
            format!("Op {index}"),
        );
        let arguments = vec!["info".to_owned(), "x".repeat(900)];
        transcript.record(SvnTranscript::entry(
            &scope,
            &arguments,
            None,
            None,
            Some("stopped"),
            0.0,
            SwiftDate::now(),
        ));
    }
    let all = transcript.entries(product, None, 1000);
    // The file is checked once it passes 256 KB (about 230 of these entries), then cut to the newest 10.
    assert!(all.len() < 400, "old entries were dropped");
    assert_eq!(all.last().map(|e| e.operation.as_str()), Some("Op 399"));
    assert!(
        transcript
            .entries(product, Some(release), 1000)
            .iter()
            .all(|e| e.release_id == Some(release))
    );
}

// Rust-only: the JSON-lines file stays readable by the Swift app and vice versa.

/// A line as Swift's `JSONEncoder` (`.iso8601` dates) writes it, including its `\/` escapes.
const SWIFT_LINE: &str = r#"{"id":"0E7C3D2A-5B1F-4C55-9F0B-6A2D8E4B7C11","date":"2026-11-26T09:30:15Z","productID":"9B2F6C1E-3D4A-4E8B-A7C5-1F0E2D3C4B5A","releaseID":"7A1B2C3D-4E5F-4061-8273-94A5B6C7D8E9","operation":"Publish","command":"svn commit --file \/tmp\/m.txt \"~\/svn\/my plugin\"","exitCode":0,"duration":1.25,"output":"Committed revision 42.","failure":null}"#;

#[test]
fn reads_entries_written_by_the_swift_app() {
    let dir = TempDir::new();
    let folder = dir.path("Transcripts");
    std::fs::create_dir_all(&folder).unwrap();
    let product = Id::parse("9B2F6C1E-3D4A-4E8B-A7C5-1F0E2D3C4B5A").unwrap();
    let minimal = r#"{"id":"1E7C3D2A-5B1F-4C55-9F0B-6A2D8E4B7C11","date":"2026-11-26T09:31:00Z","productID":"9B2F6C1E-3D4A-4E8B-A7C5-1F0E2D3C4B5A","operation":"Prepare","command":"svn info","duration":0,"output":"","failure":"Timed out after 20.0 seconds: svn info"}"#;
    std::fs::write(
        folder.join("9B2F6C1E-3D4A-4E8B-A7C5-1F0E2D3C4B5A.jsonl"),
        format!("{SWIFT_LINE}\n{minimal}\n"),
    )
    .unwrap();

    let transcript = SvnTranscript::new(&folder);
    let entries = transcript.entries(product, None, 10);
    assert_eq!(entries.len(), 2);
    let first = &entries[0];
    assert_eq!(first.date.unix_seconds(), 1_795_685_415.0);
    assert_eq!(first.release_id, Id::parse("7A1B2C3D-4E5F-4061-8273-94A5B6C7D8E9"));
    assert_eq!(first.command, r#"svn commit --file /tmp/m.txt "~/svn/my plugin""#);
    assert_eq!(first.exit_code, Some(0));
    assert_eq!(first.duration, 1.25);
    assert!(first.succeeded());
    assert_eq!((entries[1].release_id, entries[1].exit_code), (None, None));
    assert!(!entries[1].succeeded());
}

#[test]
fn writes_entries_in_the_swift_format() {
    let entry: TranscriptEntry = serde_json::from_str(SWIFT_LINE).unwrap();
    let json: serde_json::Value = serde_json::to_value(&entry).unwrap();
    assert_eq!(json["id"], "0E7C3D2A-5B1F-4C55-9F0B-6A2D8E4B7C11");
    assert_eq!(json["date"], "2026-11-26T09:30:15Z");
    assert_eq!(json["productID"], "9B2F6C1E-3D4A-4E8B-A7C5-1F0E2D3C4B5A");
    assert_eq!(json["releaseID"], "7A1B2C3D-4E5F-4061-8273-94A5B6C7D8E9");
    assert_eq!(json["exitCode"], 0);
    assert_eq!(json["duration"], 1.25);
    assert!(json.get("failure").is_none(), "absent optionals are omitted");
    let round_trip: TranscriptEntry = serde_json::from_value(json).unwrap();
    assert_eq!(round_trip, entry);
}
