//! `DiagnosticsService` has no Swift test; this checks the bundle's keys, redaction and file name.
#![allow(clippy::unwrap_used)]

mod common;

use common::{TempDir, product_with_working_copy};
use rd_model::{SwiftDate, ValidationCheckId, ValidationReport, ValidationResult};
use rd_records::{ReleaseRecord, ReleaseSource, ReleaseState, ReleaseStateMachine};
use rd_store::*;
use serde_json::Value;
use std::sync::Arc;

struct Lines;

impl LogSource for Lines {
    fn entries(&self, since: SwiftDate) -> Vec<LogEntry> {
        vec![LogEntry {
            date: since.adding(60.0),
            category: "svn".into(),
            message: "svn commit --password hunter2".into(),
        }]
    }
}

#[test]
fn exports_a_redacted_bundle_into_logs() {
    let dir = TempDir::new();
    let paths = AppPaths::new(dir.path("data"));
    let store = Arc::new(Store::open(&paths).unwrap());
    let mut product = product_with_working_copy("/tmp/wc");
    product.svn_username = Some("alice".into());
    store.save_product(&product).unwrap();
    let mut record = ReleaseRecord::new(&product, ReleaseSource::zip("/p.zip"), rd_model::ReleaseMode::Assisted);
    record.version = Some("1.2.0".into());
    record.validation = Some(ValidationReport {
        results: vec![ValidationResult::fail(
            ValidationCheckId::VersionValidity,
            "bad version",
        )],
        block_on_warnings: false,
    });
    ReleaseStateMachine::transition_at(
        &mut record,
        ReleaseState::Inspected,
        Some("ok"),
        SwiftDate::from_unix_seconds(1_795_685_400.0),
    )
    .unwrap();
    store.save_release(&record).unwrap();

    let service = DiagnosticsService::new(store.clone(), store, paths.clone()).with_log_source(Arc::new(Lines));
    // 2026-11-26T09:30:00Z
    let now = SwiftDate::from_unix_seconds(1_795_685_400.0);
    let path = service.export("1.0 (1)", Some("1.14.5"), now).unwrap();
    assert_eq!(
        path,
        paths.logs().join("ReleaseDock-diagnostics-2026-11-26T09-30-00Z.json")
    );

    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.starts_with("{\n  \"app\" : \"1.0 (1)\",\n"), "{text}");
    assert!(text.contains(r#""svnURL" : "https:\/\/plugins.svn.wordpress.org\/sample-plugin""#));
    assert!(!text.contains("alice"), "no SVN username");
    assert!(!text.contains("hunter2"), "log lines are redacted");

    let bundle: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(bundle["generatedAt"], "2026-11-26T09:30:00Z");
    assert_eq!(bundle["svn"], "1.14.5");
    let product = &bundle["products"][0];
    assert_eq!(product["hasSVNUsername"], true);
    assert_eq!(product["mode"], "assisted");
    assert_eq!(product["type"], "plugin");
    assert!(product.get("currentVersion").is_none(), "nil optionals are omitted");
    let release = &bundle["releases"][0];
    assert_eq!(release["state"], "inspected");
    assert_eq!(release["version"], "1.2.0");
    assert!(release.get("committedRevision").is_none());
    assert_eq!(
        release["transitions"][1],
        "2026-11-26T09:30:00Z detected → inspected: ok"
    );
    assert!(release["transitions"][0].as_str().unwrap().ends_with(" - → detected"));
    assert_eq!(release["blocking"][0], "Version validity: bad version");
    let log = bundle["log"][0].as_str().unwrap();
    assert!(log.starts_with("2026-11-25T09:31:00Z [svn] svn commit"), "{log}");
}
