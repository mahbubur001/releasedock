//! Ported from `DemoDataTests.swift`. Builds a demo data folder for screenshots and manual
//! UI checks:
//!
//! ```sh
//! RELEASEDOCK_MAKE_DEMO=/tmp/rd-demo cargo test -p rd-pipeline --test demo_data -- --nocapture
//! ```
//!
//! then launch the app with `RELEASEDOCK_DATA_DIR=/tmp/rd-demo/data`. Everything lives in that
//! folder: local file:// repositories, working copies and products. The folder is replaced.
#![allow(clippy::unwrap_used)]

mod common;

use common::{FaultInjectingSvn, LocalSvnRepository, StubGit, TempDir, fixtures};
use rd_keychain::InMemoryKeychain;
use rd_model::{Product, ProductType};
use rd_pipeline::{PipelineDependencies, ReleasePipelineService, ReleaseStagingService};
use rd_records::{ApprovalKind, ReleaseKind, ReleaseSource, ReleaseState};
use rd_store::{AppPaths, ProductRepository, Store};
use rd_svn::SvnService;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).unwrap();
        }
    }
}

#[tokio::test]
async fn make_demo_data() {
    let Ok(out) = std::env::var("RELEASEDOCK_MAKE_DEMO") else {
        eprintln!("skipped: set RELEASEDOCK_MAKE_DEMO");
        return;
    };
    if !LocalSvnRepository::is_available() {
        eprintln!("skipped: svn/svnadmin not installed");
        return;
    }
    let root = PathBuf::from(out);
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();

    let repo = LocalSvnRepository::persistent_at(&root.join("svn")).unwrap();
    repo.create_plugin("acme-gallery", Some("2.3.0"), "Acme Gallery")
        .unwrap();
    repo.create_theme("acme-theme", Some("1.1.0"), "Acme Theme").unwrap();

    let paths = AppPaths::new(root.join("data"));
    paths.create_all().unwrap();
    let store = Arc::new(Store::open(&paths).unwrap());
    let dev_folder = root.join("dev/acme-gallery");
    let mut gallery = Product::new(
        "Acme Gallery",
        "acme-gallery",
        ProductType::Plugin,
        &format!("{}/acme-gallery", repo.url()),
        &root.join("wc/acme-gallery").to_string_lossy(),
    );
    gallery.local_development_path = Some(dev_folder.to_string_lossy().into_owned());
    gallery.current_version = Some("2.3.0".into());
    gallery.github_repository_url = Some("https://github.com/acme/acme-gallery".into());
    gallery.svn_username = Some("acme".into());
    let mut theme = Product::new(
        "Acme Theme",
        "acme-theme",
        ProductType::Theme,
        &format!("{}/acme-theme", repo.url()),
        &root.join("wc/acme-theme").to_string_lossy(),
    );
    theme.current_version = Some("1.1.0".into());
    theme.svn_username = Some("acme".into());
    store.save_product(&gallery).unwrap();
    store.save_product(&theme).unwrap();

    let svn = Arc::new(FaultInjectingSvn::new(Arc::new(
        SvnService::new().with_command_timeout(Duration::from_secs(120)),
    )));
    let pipeline = ReleasePipelineService::new(
        PipelineDependencies::new(
            store.clone(),
            store.clone(),
            svn,
            Arc::new(StubGit::new()),
            Arc::new(InMemoryKeychain::new()),
            Arc::new(ReleaseStagingService::new(paths.clone())),
        )
        .with_generator("ReleaseDock demo"),
    );
    let source = |name: &str, version: &str, is_theme: bool, extra: &[(&str, &str)]| -> ReleaseSource {
        let src = root.join(format!("packages/{name}-{version}"));
        let _ = fs::remove_dir_all(&src);
        let tmp = TempDir::new();
        let built = if is_theme {
            fixtures::make_theme(&tmp, name, version, "Acme Theme")
        } else {
            fixtures::Plugin::new(version)
                .at(name)
                .name("Acme Gallery")
                .extra(extra)
                .make(&tmp)
        };
        copy_tree(&built, &src);
        ReleaseSource::folder(src.to_string_lossy())
    };
    let publish = async |product: &Product, src: ReleaseSource| {
        let mut record = pipeline
            .create_release(product.id, src, None, ReleaseKind::Version, None)
            .unwrap();
        record = pipeline.prepare(record.id).await.unwrap();
        pipeline.approve(record.id, ApprovalKind::User).await.unwrap();
        record = pipeline.publish(record.id).await.unwrap();
        assert_eq!(record.state, ReleaseState::Completed, "{:?}", record.errors);
    };

    // History: a theme release, a cancelled attempt and a published plugin version.
    publish(&theme, source("acme-theme", "1.2.0", true, &[])).await;
    let cancelled = pipeline
        .create_release(
            gallery.id,
            source("acme-gallery", "2.4.0", false, &[]),
            None,
            ReleaseKind::Version,
            None,
        )
        .unwrap();
    let _ = pipeline.prepare(cancelled.id).await;
    let _ = pipeline.cancel(cancelled.id).await;
    let gallery_now = store.product(gallery.id).unwrap().unwrap();
    publish(
        &gallery_now,
        source(
            "acme-gallery",
            "2.4.0",
            false,
            &[("includes/class-lightbox.php", "<?php\nfinal class Lightbox {}\n")],
        ),
    )
    .await;

    // Waiting for approval: 2.5.0 prepared and previewed.
    let waiting = pipeline
        .create_release(
            gallery.id,
            source(
                "acme-gallery",
                "2.5.0",
                false,
                &[(
                    "includes/class-lightbox.php",
                    "<?php\nfinal class Lightbox {\n    public function register(): void {}\n}\n",
                )],
            ),
            None,
            ReleaseKind::Version,
            None,
        )
        .unwrap();
    let previewed = pipeline.prepare(waiting.id).await.unwrap();
    assert_eq!(previewed.state, ReleaseState::Previewed, "{:?}", previewed.validation);

    // The development folder, one version behind, for Bump Version.
    let dev = TempDir::new();
    let plugin = fixtures::Plugin::new("2.5.0")
        .at("acme-gallery")
        .name("Acme Gallery")
        .make(&dev);
    copy_tree(&plugin, &dev_folder);
    println!("DEMO DATA: RELEASEDOCK_DATA_DIR={}", paths.root.display());
}
