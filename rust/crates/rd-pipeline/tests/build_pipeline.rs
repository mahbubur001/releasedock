//! `BuildPipelineTests` from `BuildTests.swift` (the build service itself is tested in
//! rd-integrations).
#![allow(clippy::unwrap_used)]

mod common;

use common::{Harness, LocalSvnRepository, StubGit, TempDir, fixtures, product};
use rd_integrations::BuildService;
use rd_model::ProductType;
use rd_pipeline::ReleasePipelineService;
#[cfg(unix)]
use rd_records::ApprovalKind;
use rd_records::{ReleaseKind, ReleaseSource, ReleaseState};
use rd_store::{AppPaths, ProductRepository, ReleaseRepository, Store};
use rd_svn::SvnService;
use std::sync::Arc;
use std::time::Duration;

struct Setup {
    repo: LocalSvnRepository,
    work: TempDir,
    store: Arc<Store>,
    pipeline: ReleasePipelineService,
}

fn setup() -> Option<Setup> {
    let repo = LocalSvnRepository::new_or_skip()?;
    let work = TempDir::new();
    let store = Arc::new(Store::in_memory().unwrap());
    let paths = AppPaths::new(work.path("data"));
    paths.create_all().unwrap();
    let svn = Arc::new(SvnService::new().with_command_timeout(Duration::from_secs(60)));
    let deps = Harness::dependencies(&store, svn, &paths, Arc::new(StubGit::new()))
        .with_builder(Some(Arc::new(BuildService::new())));
    Some(Setup {
        repo,
        work,
        store,
        pipeline: ReleasePipelineService::new(deps),
    })
}

fn register(s: &Setup, build_command: &str) -> rd_model::Product {
    s.repo
        .create_plugin("sample-plugin", Some("1.0.0"), "Sample Plugin")
        .unwrap();
    let mut product = product(
        "Sample Plugin",
        "sample-plugin",
        ProductType::Plugin,
        &format!("{}/sample-plugin", s.repo.url()),
        &s.work.path("wc-plugin"),
        Some("1.0.0"),
    );
    product.build_command = Some(build_command.into());
    s.store.save_product(&product).unwrap();
    product
}

/// The build command is POSIX shell (`mkdir -p`, `>`), so this runs on Unix.
#[cfg(unix)]
#[tokio::test]
async fn folder_release_ships_the_built_copy() {
    let Some(s) = setup() else { return };
    let product = register(&s, "mkdir -p build && echo compiled > build/app.js");
    let source = fixtures::Plugin::new("1.1.0").at("dev/sample-plugin").make(&s.work);

    let mut record = s
        .pipeline
        .create_release(
            product.id,
            ReleaseSource::folder(source.to_string_lossy()),
            None,
            ReleaseKind::Version,
            None,
        )
        .unwrap();
    record = s.pipeline.prepare(record.id).await.unwrap();
    assert_eq!(
        record.state,
        ReleaseState::Previewed,
        "{:?} {:?}",
        record.validation,
        record.errors
    );
    assert_eq!(record.build.as_ref().map(|b| b.exit_code), Some(0));
    assert!(
        record
            .preview
            .as_ref()
            .unwrap()
            .changes
            .iter()
            .any(|c| c.path == "trunk/build/app.js")
    );
    assert!(!source.join("build").exists(), "dev folder untouched");
    s.pipeline.approve(record.id, ApprovalKind::User).await.unwrap();
    record = s.pipeline.publish(record.id).await.unwrap();
    assert_eq!(record.state, ReleaseState::Completed, "{:?}", record.errors);
    assert_eq!(record.verification.as_ref().map(|v| v.is_clean()), Some(true));
    assert_eq!(
        s.repo
            .run(&[
                "cat",
                &format!("{}/sample-plugin/tags/1.1.0/build/app.js", s.repo.url())
            ])
            .unwrap(),
        "compiled\n"
    );
}

#[tokio::test]
async fn build_failure_fails_the_release() {
    let Some(s) = setup() else { return };
    let product = register(&s, "exit 1");
    let source = fixtures::Plugin::new("1.1.0").at("dev/sample-plugin").make(&s.work);
    let record = s
        .pipeline
        .create_release(
            product.id,
            ReleaseSource::folder(source.to_string_lossy()),
            None,
            ReleaseKind::Version,
            None,
        )
        .unwrap();
    let error = s.pipeline.prepare(record.id).await.unwrap_err();
    assert!(
        matches!(error, rd_pipeline::ReleaseError::BuildFailed { exit_code: 1, .. }),
        "{error:?}"
    );
    assert_eq!(
        s.store.release(record.id).unwrap().map(|r| r.state),
        Some(ReleaseState::Failed)
    );
}
