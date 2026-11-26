//! `AppPaths` has no Swift test of its own; these pin the layout the Swift app uses.
#![allow(clippy::unwrap_used)]

mod common;

use common::TempDir;
use rd_model::{Id, ProductType};
use rd_store::*;
use std::collections::HashMap;
use std::path::PathBuf;

#[test]
fn layout_matches_the_swift_data_folder() {
    let paths = AppPaths::new("/data");
    assert_eq!(
        paths.database_file(),
        PathBuf::from("/data/Database/releasedock.sqlite")
    );
    assert_eq!(paths.backups(), PathBuf::from("/data/Backups"));
    assert_eq!(paths.logs(), PathBuf::from("/data/Logs"));
    assert_eq!(paths.transcripts(), PathBuf::from("/data/Transcripts"));
    let id = Id::parse("7b129e3f-70d3-44a1-bb79-058e525cde3b").unwrap();
    assert_eq!(
        paths.staging_directory(id),
        PathBuf::from("/data/Staging/7B129E3F-70D3-44A1-BB79-058E525CDE3B")
    );
    assert_eq!(
        paths.release_directory("acme", "1.4.1", id),
        PathBuf::from("/data/Releases/acme/1.4.1-7B129E3F")
    );
    assert_eq!(
        paths.default_working_copy("acme", ProductType::Plugin),
        PathBuf::from("/data/Products/plugin-acme/svn")
    );
}

#[test]
fn absolute_override_wins_and_relative_override_is_ignored() {
    let dir = TempDir::new();
    let absolute = dir.path("profile");
    let env = HashMap::from([(DATA_DIR_VARIABLE.to_owned(), absolute.to_string_lossy().into_owned())]);
    assert_eq!(AppPaths::default_root_with(&env).unwrap(), absolute);

    let env = HashMap::from([(DATA_DIR_VARIABLE.to_owned(), "relative/profile".to_owned())]);
    let default = AppPaths::default_root_with(&HashMap::new()).unwrap();
    assert_eq!(AppPaths::default_root_with(&env).unwrap(), default);
}

#[test]
fn default_root_is_the_platform_data_folder() {
    let root = AppPaths::default_root_with(&HashMap::new()).unwrap();
    assert!(root.ends_with("ReleaseDock/ReleaseDockData"));
    #[cfg(target_os = "macos")]
    assert_eq!(
        root,
        dirs::home_dir()
            .unwrap()
            .join("Library/Application Support/ReleaseDock/ReleaseDockData")
    );
    #[cfg(windows)]
    assert_eq!(
        root,
        PathBuf::from(std::env::var("APPDATA").unwrap()).join("ReleaseDock\\ReleaseDockData")
    );
    #[cfg(target_os = "linux")]
    assert!(root.starts_with(dirs::data_dir().unwrap()));
}

#[test]
fn create_all_makes_every_folder() {
    let dir = TempDir::new();
    let paths = AppPaths::new(dir.path("data"));
    paths.create_all().unwrap();
    for folder in [
        paths.products(),
        paths.staging(),
        paths.releases(),
        paths.backups(),
        paths.logs(),
        paths.database(),
    ] {
        assert!(folder.is_dir(), "{}", folder.display());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(paths.staging()).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
    }
}
