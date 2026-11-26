//! `CodeCheckPipelineTests` from `CodeCheckTests.swift` (parsing, the service and the
//! validation rules are tested in rd-integrations and rd-validation).
#![allow(clippy::unwrap_used)]

mod common;

use common::{Harness, LocalSvnRepository, StubCodeChecker, StubGit, TempDir, fixtures, product};
use rd_model::{ProductType, ValidationCheckId, ValidationSeverity};
use rd_pipeline::ReleasePipelineService;
use rd_pipeline::working_copy::local_changes;
use rd_records::{CodeCheckFinding, CodeCheckReport, CodeCheckTool, ReleaseKind, ReleaseSource, ReleaseState};
use rd_store::{AppPaths, ProductRepository, Store};
use rd_svn::SvnService;
use std::sync::Arc;
use std::time::Duration;

#[tokio::test]
async fn syntax_error_stops_before_the_working_copy_is_touched() {
    let Some(repo) = LocalSvnRepository::new_or_skip() else {
        return;
    };
    let work = TempDir::new();
    let store = Arc::new(Store::in_memory().unwrap());
    let paths = AppPaths::new(work.path("data"));
    paths.create_all().unwrap();
    let svn = Arc::new(SvnService::new().with_command_timeout(Duration::from_secs(60)));
    let failing = CodeCheckReport {
        php_version: Some("8.5.7".into()),
        linted_files: 3,
        findings: vec![CodeCheckFinding::new(
            CodeCheckTool::PhpLint,
            ValidationSeverity::Error,
            Some("includes/class-sample.php".into()),
            Some(2),
            None,
            "Parse error: syntax error",
        )],
        ..CodeCheckReport::default()
    };
    let deps = Harness::dependencies(&store, svn.clone(), &paths, Arc::new(StubGit::new())).with_code_checker(Some(
        Arc::new(StubCodeChecker {
            report: failing.clone(),
        }),
    ));
    let pipeline = ReleasePipelineService::new(deps);
    repo.create_plugin("sample-plugin", Some("1.0.0"), "Sample Plugin")
        .unwrap();
    let product = product(
        "Sample Plugin",
        "sample-plugin",
        ProductType::Plugin,
        &format!("{}/sample-plugin", repo.url()),
        &work.path("wc-plugin"),
        Some("1.0.0"),
    );
    store.save_product(&product).unwrap();
    let src = TempDir::new();
    let plugin = fixtures::make_plugin(&src, "1.1.0");

    let mut record = pipeline
        .create_release(
            product.id,
            ReleaseSource::folder(plugin.to_string_lossy()),
            None,
            ReleaseKind::Version,
            None,
        )
        .unwrap();
    record = pipeline.prepare(record.id).await.unwrap();
    assert_eq!(record.state, ReleaseState::Failed);
    assert!(!record.working_copy_modified, "nothing was applied");
    assert_eq!(record.code_checks.as_ref(), Some(&failing));
    assert!(
        record
            .validation
            .as_ref()
            .unwrap()
            .blocking_results()
            .iter()
            .any(|r| r.check == ValidationCheckId::PhpSyntax)
    );
    let changes = local_changes(&product.working_copy_dir(), svn.as_ref()).await.unwrap();
    assert!(changes.is_empty(), "{changes:?}");
    assert!(record.preview.is_none());
}
