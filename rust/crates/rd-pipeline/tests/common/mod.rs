//! Test helpers ported from `Tests/ReleaseDockCoreTests/Support/TestSupport.swift`
//! (`TempDir`, `Fixtures`, `LocalSVNRepository`, `FaultInjectingSVN`, `RecordingNotifications`,
//! `StubGit`) plus the stubs of the pipeline test files (`StubWordPressOrg`,
//! `StubCodeChecker`, `StubGitHubReleaser`) and the `setUp` of `PipelineIntegrationTests`
//! ([`Harness`]). `/usr/bin/zip` is replaced by [`zip_folder`] (from rd-inspection's tests).
#![allow(dead_code, clippy::unwrap_used)]

use async_trait::async_trait;
use flate2::Compression;
use flate2::write::DeflateEncoder;
use rd_integrations::{
    CodeChecking, CreateRelease, GitHubError, GitHubRelease, GitHubReleasing, GitHubRepository, GitServicing,
    WordPressOrgError, WordPressOrgServicing,
};
use rd_keychain::InMemoryKeychain;
use rd_model::{Id, Product, ProductType, ReleaseMode};
use rd_pipeline::{NotificationServicing, PipelineDependencies, ReleasePipelineService, ReleaseStagingService};
use rd_records::{CodeCheckReport, DirectoryListing, GitRepositoryState, ReleaseSource};
use rd_store::{AppPaths, ProductRepository, Store};
use rd_support::hashing::Crc32;
use rd_svn::{
    ProcessRequest, ProcessRunnerError, SvnConflictChoice, SvnCredentials, SvnDepth, SvnError, SvnErrorKind, SvnInfo,
    SvnInstallation, SvnListEntry, SvnLogEntry, SvnService, SvnServicing, SvnStatusEntry, SvnTranscript,
};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

// MARK: TempDir

/// Temporary directory removed on drop, with symlinks resolved (macOS `/var` → `/private/var`)
/// so paths match what svn reports.
pub struct TempDir {
    _dir: tempfile::TempDir,
    pub url: PathBuf,
}

impl TempDir {
    pub fn new() -> Self {
        let dir = tempfile::Builder::new().prefix("ReleaseDockTests-").tempdir().unwrap();
        let url = dir.path().canonicalize().unwrap();
        Self { _dir: dir, url }
    }

    pub fn path(&self, relative: &str) -> PathBuf {
        self.url.join(relative)
    }

    pub fn write(&self, relative: &str, contents: impl AsRef<[u8]>) -> PathBuf {
        let file = self.path(relative);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, contents).unwrap();
        file
    }

    pub fn mkdir(&self, relative: &str) {
        fs::create_dir_all(self.path(relative)).unwrap();
    }
}

// MARK: Fixtures

pub mod fixtures {
    use super::TempDir;
    use std::path::PathBuf;

    pub fn plugin_main_file(name: &str, version: &str, text_domain: &str) -> String {
        format!(
            "<?php\n/**\n * Plugin Name: {name}\n * Description: A sample plugin.\n * Version: {version}\n * Requires at least: 6.0\n * Requires PHP: 7.4\n * Author: Test Author\n * License: GPL-2.0-or-later\n * Text Domain: {text_domain}\n */\ndefined( 'ABSPATH' ) || exit;"
        )
    }

    pub fn plugin_readme(name: &str, stable_tag: &str, version: Option<&str>) -> String {
        let version = version.unwrap_or(stable_tag);
        format!(
            "=== {name} ===\nContributors: testauthor\nTags: testing, release\nRequires at least: 6.0\nTested up to: 6.8\nRequires PHP: 7.4\nStable tag: {stable_tag}\nLicense: GPLv2 or later\nLicense URI: https://www.gnu.org/licenses/gpl-2.0.html\n\nA sample plugin.\n\n== Description ==\n\nDoes sample things.\n\n== Changelog ==\n\n= {version} =\n* Release.\n\n= 0.9.0 =\n* Earlier."
        )
    }

    /// `Fixtures.makePlugin(in:at:version:stableTag:name:extraFiles:)`.
    pub struct Plugin<'a> {
        pub at: &'a str,
        pub version: &'a str,
        pub stable_tag: Option<&'a str>,
        pub name: &'a str,
        pub extra: Vec<(&'a str, String)>,
    }

    impl<'a> Plugin<'a> {
        pub fn new(version: &'a str) -> Self {
            Self {
                at: "sample-plugin",
                version,
                stable_tag: None,
                name: "Sample Plugin",
                extra: Vec::new(),
            }
        }

        pub fn at(mut self, at: &'a str) -> Self {
            self.at = at;
            self
        }

        pub fn stable_tag(mut self, stable_tag: Option<&'a str>) -> Self {
            self.stable_tag = stable_tag;
            self
        }

        pub fn name(mut self, name: &'a str) -> Self {
            self.name = name;
            self
        }

        pub fn extra(mut self, files: &[(&'a str, &str)]) -> Self {
            self.extra.extend(files.iter().map(|(p, c)| (*p, (*c).to_owned())));
            self
        }

        pub fn make(self, dir: &TempDir) -> PathBuf {
            let relative = self.at;
            dir.write(
                &format!("{relative}/sample-plugin.php"),
                plugin_main_file(self.name, self.version, "sample-plugin"),
            );
            dir.write(
                &format!("{relative}/readme.txt"),
                plugin_readme(self.name, self.stable_tag.unwrap_or(self.version), Some(self.version)),
            );
            dir.write(
                &format!("{relative}/includes/class-sample.php"),
                "<?php\nclass Sample {}\n",
            );
            dir.write(
                &format!("{relative}/assets/js/app.js"),
                format!("console.log('v{}');\n", self.version),
            );
            for (path, contents) in &self.extra {
                dir.write(&format!("{relative}/{path}"), contents);
            }
            dir.path(relative)
        }
    }

    pub fn make_plugin(dir: &TempDir, version: &str) -> PathBuf {
        Plugin::new(version).make(dir)
    }

    pub fn theme_style(name: &str, version: &str) -> String {
        format!(
            "/*\nTheme Name: {name}\nAuthor: Test Author\nDescription: A sample theme.\nVersion: {version}\nRequires at least: 6.0\nTested up to: 6.8\nRequires PHP: 7.4\nLicense: GNU General Public License v2 or later\nLicense URI: http://www.gnu.org/licenses/gpl-2.0.html\nText Domain: sample-theme\nTags: blog, one-column\n*/"
        )
    }

    pub fn make_theme(dir: &TempDir, relative: &str, version: &str, name: &str) -> PathBuf {
        dir.write(&format!("{relative}/style.css"), theme_style(name, version));
        dir.write(&format!("{relative}/index.php"), "<?php get_header(); ?>\n");
        dir.write(&format!("{relative}/functions.php"), format!("<?php // v{version}\n"));
        dir.write(&format!("{relative}/screenshot.png"), "PNG-DATA");
        dir.write(
            &format!("{relative}/readme.txt"),
            format!(
                "=== {name} ===\nContributors: testauthor\nRequires at least: 6.0\nTested up to: 6.8\nRequires PHP: 7.4\nLicense: GPLv2 or later\nLicense URI: http://www.gnu.org/licenses/gpl-2.0.html\n\n== Changelog ==\n\n= {version} =\n* Release."
            ),
        );
        dir.path(relative)
    }
}

// MARK: ZIP

fn le16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn le32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

/// Zips `folder` (including the folder itself as the archive root), like `zip -q -r -y`.
pub fn zip_folder(folder: &Path, destination: &Path) {
    let root_name = folder.file_name().unwrap().to_str().unwrap().to_owned();
    let mut items: BTreeMap<String, PathBuf> = BTreeMap::new();
    collect(folder, &root_name, &mut items);

    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, path) in &items {
        let metadata = fs::symlink_metadata(path).unwrap();
        let (name, data, mode) = if metadata.file_type().is_symlink() {
            let target = fs::read_link(path).unwrap();
            (
                name.clone(),
                target.to_string_lossy().into_owned().into_bytes(),
                0o120777,
            )
        } else if metadata.is_dir() {
            (format!("{name}/"), Vec::new(), 0o040755)
        } else {
            (name.clone(), fs::read(path).unwrap(), 0o100644)
        };
        let (method, stored) = if data.is_empty() || mode != 0o100644 {
            (0u16, data.clone())
        } else {
            let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
            encoder.write_all(&data).unwrap();
            (8u16, encoder.finish().unwrap())
        };
        let crc = Crc32::of(&data);
        let offset = out.len() as u32;
        le32(&mut out, 0x0403_4B50);
        for v in [20, 0x0800, method, 0, 0] {
            le16(&mut out, v);
        }
        le32(&mut out, crc);
        le32(&mut out, stored.len() as u32);
        le32(&mut out, data.len() as u32);
        le16(&mut out, name.len() as u16);
        le16(&mut out, 0);
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(&stored);

        le32(&mut central, 0x0201_4B50);
        for v in [0x031E, 20, 0x0800, method, 0, 0] {
            le16(&mut central, v);
        }
        le32(&mut central, crc);
        le32(&mut central, stored.len() as u32);
        le32(&mut central, data.len() as u32);
        for v in [name.len() as u16, 0, 0, 0, 0] {
            le16(&mut central, v);
        }
        le32(&mut central, mode << 16);
        le32(&mut central, offset);
        central.extend_from_slice(name.as_bytes());
    }
    let cd_offset = out.len() as u32;
    out.extend_from_slice(&central);
    le32(&mut out, 0x0605_4B50);
    for v in [0, 0, items.len() as u16, items.len() as u16] {
        le16(&mut out, v);
    }
    le32(&mut out, central.len() as u32);
    le32(&mut out, cd_offset);
    le16(&mut out, 0);
    fs::write(destination, out).unwrap();
}

fn collect(path: &Path, name: &str, items: &mut BTreeMap<String, PathBuf>) {
    items.insert(name.to_owned(), path.to_path_buf());
    let metadata = fs::symlink_metadata(path).unwrap();
    if metadata.is_dir() {
        for child in fs::read_dir(path).unwrap() {
            let child = child.unwrap();
            let child_name = format!("{name}/{}", child.file_name().to_str().unwrap());
            collect(&child.path(), &child_name, items);
        }
    }
}

/// Every path beneath `root`, sorted (`FileManager.subpathsOfDirectory`).
pub fn subpaths(root: &Path) -> Vec<String> {
    let mut items = BTreeMap::new();
    collect(root, "", &mut items);
    items
        .into_keys()
        .filter(|k| !k.is_empty())
        .map(|k| k.trim_start_matches('/').to_owned())
        .collect()
}

/// Entry names of a ZIP archive (`zipinfo -1`), read with ReleaseDock's own ZIP reader.
pub fn zip_entries(path: &Path) -> Vec<String> {
    rd_inspection::ZipArchive::open(path)
        .unwrap()
        .entries
        .iter()
        .filter_map(|e| e.name.clone())
        .collect()
}

// MARK: LocalSvnRepository

fn find_tool(name: &str) -> Option<PathBuf> {
    let unix = [
        format!("/opt/homebrew/bin/{name}"),
        format!("/usr/local/bin/{name}"),
        format!("/opt/local/bin/{name}"),
        format!("/usr/bin/{name}"),
    ];
    unix.iter()
        .map(PathBuf::from)
        .find(|p| rd_svn::process_runner::is_executable_file(p))
        .or_else(|| which::which(name).ok())
}

pub fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|v| (*v).to_owned()).collect()
}

/// A local Subversion repository laid out like WordPress.org, using svnadmin and file:// URLs.
pub struct LocalSvnRepository {
    pub dir: TempDir,
    pub root: PathBuf,
    pub svn: PathBuf,
}

impl LocalSvnRepository {
    pub fn svn_executable() -> Option<PathBuf> {
        SvnService::candidate_executables(None)
            .into_iter()
            .find(|p| rd_svn::process_runner::is_executable_file(p))
    }

    pub fn is_available() -> bool {
        Self::svn_executable().is_some() && find_tool("svnadmin").is_some()
    }

    /// `None` (and a note on stderr) when svn or svnadmin is not installed, so tests skip.
    pub fn new_or_skip() -> Option<Self> {
        if !Self::is_available() {
            eprintln!("skipped: svn/svnadmin not installed");
            return None;
        }
        Some(Self::new().expect("create local repository"))
    }

    pub fn new() -> Result<Self, String> {
        let dir = TempDir::new();
        let root = dir.path("repo");
        Self::create(&root)?;
        Ok(Self {
            dir,
            root,
            svn: Self::svn_executable().ok_or("svn not installed")?,
        })
    }

    /// A repository at `root` that outlives the test (demo data).
    pub fn persistent_at(root: &Path) -> Result<Self, String> {
        fs::create_dir_all(root.parent().ok_or("no parent")?).map_err(|e| e.to_string())?;
        Self::create(root)?;
        Ok(Self {
            dir: TempDir::new(),
            root: root.to_path_buf(),
            svn: Self::svn_executable().ok_or("svn not installed")?,
        })
    }

    fn create(root: &Path) -> Result<(), String> {
        let admin = find_tool("svnadmin").ok_or("svnadmin not installed")?;
        Self::run_tool(&admin, &["create".into(), root.to_string_lossy().into_owned()])?;
        Ok(())
    }

    /// `file://` URL of the repository root (`file:///C:/…` on Windows).
    pub fn url(&self) -> String {
        let path = self.root.to_string_lossy().replace('\\', "/");
        if path.starts_with('/') {
            format!("file://{path}")
        } else {
            format!("file:///{path}")
        }
    }

    pub fn run(&self, args: &[&str]) -> Result<String, String> {
        let mut all = strings(args);
        all.push("--non-interactive".into());
        Self::run_tool(&self.svn, &all)
    }

    pub fn run_tool(exe: &Path, args: &[String]) -> Result<String, String> {
        let output = Command::new(exe)
            .args(args)
            .env_clear()
            .envs(ProcessRequest::default_environment(
                &rd_svn::process_runner::home_directory(),
            ))
            .stdin(Stdio::null())
            .output()
            .map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).into_owned());
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    /// Creates `/<slug>/{trunk,tags,assets}` and optionally publishes an initial version.
    pub fn create_plugin(&self, slug: &str, initial_version: Option<&str>, name: &str) -> Result<(), String> {
        let url = self.url();
        self.run(&[
            "mkdir",
            "--parents",
            "-m",
            &format!("Create {slug}"),
            &format!("{url}/{slug}/trunk"),
            &format!("{url}/{slug}/tags"),
            &format!("{url}/{slug}/assets"),
        ])?;
        let Some(initial_version) = initial_version else {
            return Ok(());
        };
        let src = TempDir::new();
        let plugin = fixtures::Plugin::new(initial_version).at("p").name(name).make(&src);
        let wc = self.dir.path(&format!("seed-{}", Id::new()));
        self.run(&["checkout", &format!("{url}/{slug}"), &wc.to_string_lossy()])?;
        let trunk = wc.join("trunk");
        copy_dir_contents(&plugin, &trunk).map_err(|e| e.to_string())?;
        self.run(&["add", "--force", &trunk.to_string_lossy()])?;
        self.run(&[
            "copy",
            &trunk.to_string_lossy(),
            &wc.join(format!("tags/{initial_version}")).to_string_lossy(),
        ])?;
        self.run(&[
            "commit",
            "-m",
            &format!("Initial {initial_version}"),
            &wc.to_string_lossy(),
        ])?;
        Ok(())
    }

    pub fn create_theme(&self, slug: &str, initial_version: Option<&str>, name: &str) -> Result<(), String> {
        let url = self.url();
        self.run(&[
            "mkdir",
            "--parents",
            "-m",
            &format!("Create {slug}"),
            &format!("{url}/{slug}"),
        ])?;
        let Some(initial_version) = initial_version else {
            return Ok(());
        };
        let src = TempDir::new();
        let theme = fixtures::make_theme(&src, "t", initial_version, name);
        self.run(&[
            "import",
            "-m",
            &format!("Initial {initial_version}"),
            &theme.to_string_lossy(),
            &format!("{url}/{slug}/{initial_version}"),
        ])?;
        Ok(())
    }
}

fn copy_dir_contents(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_contents(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

// MARK: FaultInjectingSvn

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommitFault {
    None,
    /// Commit really happens, then the client reports an uncertain outcome (e.g. timeout after the server committed).
    UncertainAfterCommit,
    /// The client times out before anything reaches the server.
    UncertainWithoutCommit,
    AuthenticationFailure,
    NetworkFailureBeforeCommit,
    HookRejection,
}

/// Wraps a real `SvnServicing` and injects failures into selected operations.
pub struct FaultInjectingSvn {
    pub base: Arc<dyn SvnServicing>,
    commit_fault: Mutex<CommitFault>,
    pub fail_info_with_network_error: AtomicBool,
    pub fail_log_with_network_error: AtomicBool,
    pub cached_credentials: AtomicBool,
    commit_count: AtomicUsize,
}

fn failure(kind: SvnErrorKind, code: &str, message: &str, command: &str) -> SvnError {
    SvnError::CommandFailed {
        kind,
        codes: vec![code.into()],
        message: message.into(),
        command: command.into(),
    }
}

impl FaultInjectingSvn {
    pub fn new(base: Arc<dyn SvnServicing>) -> Self {
        Self {
            base,
            commit_fault: Mutex::new(CommitFault::None),
            fail_info_with_network_error: AtomicBool::new(false),
            fail_log_with_network_error: AtomicBool::new(false),
            cached_credentials: AtomicBool::new(true),
            commit_count: AtomicUsize::new(0),
        }
    }

    pub fn set_commit_fault(&self, fault: CommitFault) {
        *self.commit_fault.lock().unwrap() = fault;
    }

    pub fn set_fail_info(&self, value: bool) {
        self.fail_info_with_network_error.store(value, Ordering::SeqCst);
    }

    pub fn set_fail_log(&self, value: bool) {
        self.fail_log_with_network_error.store(value, Ordering::SeqCst);
    }

    pub fn set_cached_credentials(&self, value: bool) {
        self.cached_credentials.store(value, Ordering::SeqCst);
    }

    pub fn commit_count(&self) -> usize {
        self.commit_count.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl SvnServicing for FaultInjectingSvn {
    async fn detect_installation(&self) -> Result<SvnInstallation, SvnError> {
        self.base.detect_installation().await
    }
    async fn info(&self, target: &str) -> Result<SvnInfo, SvnError> {
        if self.fail_info_with_network_error.load(Ordering::SeqCst) {
            return Err(failure(
                SvnErrorKind::Network,
                "E170013",
                "Unable to connect",
                "svn info",
            ));
        }
        self.base.info(target).await
    }
    async fn exists(&self, target: &str) -> Result<bool, SvnError> {
        if self.fail_info_with_network_error.load(Ordering::SeqCst) {
            self.info(target).await?;
        }
        self.base.exists(target).await
    }
    async fn list(&self, url: &str, recursive: bool) -> Result<Vec<SvnListEntry>, SvnError> {
        self.base.list(url, recursive).await
    }
    async fn cat(&self, url: &str) -> Result<Vec<u8>, SvnError> {
        self.base.cat(url).await
    }
    async fn checkout(&self, url: &str, path: &Path, depth: SvnDepth) -> Result<(), SvnError> {
        self.base.checkout(url, path, depth).await
    }
    async fn update(&self, path: &Path, set_depth: Option<SvnDepth>) -> Result<i64, SvnError> {
        self.base.update(path, set_depth).await
    }
    async fn status(&self, path: &Path, show_updates: bool) -> Result<Vec<SvnStatusEntry>, SvnError> {
        self.base.status(path, show_updates).await
    }
    async fn diff(&self, path: &Path) -> Result<String, SvnError> {
        self.base.diff(path).await
    }
    async fn add(&self, paths: &[PathBuf]) -> Result<(), SvnError> {
        self.base.add(paths).await
    }
    async fn delete(&self, paths: &[PathBuf]) -> Result<(), SvnError> {
        self.base.delete(paths).await
    }
    async fn copy(&self, source: &Path, destination: &Path) -> Result<(), SvnError> {
        self.base.copy(source, destination).await
    }
    async fn revert(&self, path: &Path) -> Result<(), SvnError> {
        self.base.revert(path).await
    }
    async fn cleanup(&self, path: &Path) -> Result<(), SvnError> {
        self.base.cleanup(path).await
    }
    async fn resolve(&self, paths: &[PathBuf], accept: SvnConflictChoice) -> Result<(), SvnError> {
        self.base.resolve(paths, accept).await
    }
    async fn set_property(&self, name: &str, value: &str, paths: &[PathBuf]) -> Result<(), SvnError> {
        self.base.set_property(name, value, paths).await
    }
    async fn commit(
        &self,
        paths: &[PathBuf],
        message: &str,
        credentials: Option<&SvnCredentials>,
    ) -> Result<i64, SvnError> {
        self.commit_count.fetch_add(1, Ordering::SeqCst);
        let fault = *self.commit_fault.lock().unwrap();
        match fault {
            CommitFault::None => self.base.commit(paths, message, credentials).await,
            CommitFault::UncertainAfterCommit => {
                self.base.commit(paths, message, credentials).await?;
                Err(SvnError::CommitOutcomeUncertain(
                    "simulated timeout after the server committed".into(),
                ))
            }
            CommitFault::UncertainWithoutCommit => Err(SvnError::CommitOutcomeUncertain(
                "simulated timeout before reaching the server".into(),
            )),
            CommitFault::AuthenticationFailure => Err(failure(
                SvnErrorKind::AuthenticationFailed,
                "E170001",
                "Authorization failed",
                "svn commit",
            )),
            CommitFault::NetworkFailureBeforeCommit => {
                Err(SvnError::CommitOutcomeUncertain("Unable to connect".into()))
            }
            CommitFault::HookRejection => Err(failure(
                SvnErrorKind::HookRejected,
                "E165001",
                "Commit blocked by pre-commit hook",
                "svn commit",
            )),
        }
    }
    async fn log(&self, url: &str, limit: usize) -> Result<Vec<SvnLogEntry>, SvnError> {
        if self.fail_log_with_network_error.load(Ordering::SeqCst) {
            return Err(failure(
                SvnErrorKind::Network,
                "E170013",
                "Unable to connect",
                "svn log",
            ));
        }
        self.base.log(url, limit).await
    }
    async fn export(&self, url: &str, path: &Path) -> Result<(), SvnError> {
        self.base.export(url, path).await
    }
    async fn has_cached_credentials(&self, _repository_url: &str) -> Result<bool, SvnError> {
        Ok(self.cached_credentials.load(Ordering::SeqCst))
    }
}

// MARK: Stubs

#[derive(Default)]
pub struct RecordingNotifications {
    titles: Mutex<Vec<String>>,
}

impl RecordingNotifications {
    pub fn titles(&self) -> Vec<String> {
        self.titles.lock().unwrap().clone()
    }
}

#[async_trait]
impl NotificationServicing for RecordingNotifications {
    async fn request_authorization(&self) -> bool {
        true
    }
    async fn notify(&self, title: &str, _body: &str, _identifier: &str) {
        self.titles.lock().unwrap().push(title.to_owned());
    }
}

pub struct StubGit {
    pub state: GitRepositoryState,
}

impl StubGit {
    pub fn new() -> Self {
        Self {
            state: GitRepositoryState::not_a_repository(),
        }
    }

    pub fn with_state(state: GitRepositoryState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl GitServicing for StubGit {
    async fn is_available(&self) -> bool {
        true
    }
    async fn state(&self, _directory: &Path) -> Result<GitRepositoryState, ProcessRunnerError> {
        Ok(self.state.clone())
    }
}

/// `StubWordPressOrg` from `DirectoryStatusTests.swift`.
pub struct StubWordPressOrg {
    listing: Mutex<Option<DirectoryListing>>,
    requests: AtomicUsize,
}

impl StubWordPressOrg {
    pub fn new(listing: Option<DirectoryListing>) -> Self {
        Self {
            listing: Mutex::new(listing),
            requests: AtomicUsize::new(0),
        }
    }

    pub fn set_listing(&self, listing: Option<DirectoryListing>) {
        *self.listing.lock().unwrap() = listing;
    }

    pub fn requests(&self) -> usize {
        self.requests.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl WordPressOrgServicing for StubWordPressOrg {
    async fn latest_word_press_version(&self) -> Result<String, WordPressOrgError> {
        Ok("7.1.5".into())
    }
    async fn directory_listing(
        &self,
        _slug: &str,
        _product_type: ProductType,
    ) -> Result<Option<DirectoryListing>, WordPressOrgError> {
        self.requests.fetch_add(1, Ordering::SeqCst);
        Ok(self.listing.lock().unwrap().clone())
    }
}

/// `StubCodeChecker` from `CodeCheckTests.swift`: pipeline tests do not depend on a local PHP.
pub struct StubCodeChecker {
    pub report: CodeCheckReport,
}

#[async_trait]
impl CodeChecking for StubCodeChecker {
    async fn check(
        &self,
        _root: &Path,
        _php_files: &[String],
        _product: &Product,
        _requires_php: Option<&str>,
    ) -> CodeCheckReport {
        self.report.clone()
    }
}

#[derive(Clone, Debug)]
pub struct Created {
    pub tag: String,
    pub commit: String,
    pub body: String,
    pub prerelease: bool,
}

/// `StubGitHubReleaser` from `GitHubReleaseTests.swift`; uploads record the ZIP's entries.
#[derive(Default)]
pub struct StubGitHubReleaser {
    pub created: Mutex<Vec<Created>>,
    pub uploads: Mutex<Vec<(String, Vec<String>)>>,
}

impl StubGitHubReleaser {
    pub fn release(tag: &str) -> GitHubRelease {
        let json = format!(
            r#"{{"id": 7, "tag_name": "{tag}", "name": null, "body": null, "draft": false, "prerelease": false,
 "html_url": "https://github.com/acme/acme-gallery/releases/tag/{tag}", "published_at": null, "assets": [],
 "upload_url": "https://uploads.github.com/repos/acme/acme-gallery/releases/7/assets{{?name,label}}"}}"#
        );
        GitHubRelease::decode(json.as_bytes()).expect("stub release")
    }
}

#[async_trait]
impl GitHubReleasing for StubGitHubReleaser {
    async fn commit_exists(&self, _: &str, _: &GitHubRepository, _: &str) -> Result<bool, GitHubError> {
        Ok(true)
    }
    async fn release(&self, _: &str, _: &GitHubRepository, _: &str) -> Result<Option<GitHubRelease>, GitHubError> {
        Ok(None)
    }
    async fn create_release(
        &self,
        _: &GitHubRepository,
        release: &CreateRelease<'_>,
        _: &str,
    ) -> Result<GitHubRelease, GitHubError> {
        self.created.lock().unwrap().push(Created {
            tag: release.tag.into(),
            commit: release.commit.into(),
            body: release.body.into(),
            prerelease: release.prerelease,
        });
        Ok(Self::release(release.tag))
    }
    async fn upload_asset(&self, file: &Path, name: &str, _: &GitHubRelease, _: &str) -> Result<(), GitHubError> {
        // Record the archive's entries to check its layout.
        self.uploads.lock().unwrap().push((name.into(), zip_entries(file)));
        Ok(())
    }
}

// MARK: Harness (PipelineIntegrationTests.setUp)

pub fn product(
    name: &str,
    slug: &str,
    product_type: ProductType,
    url: &str,
    working_copy: &Path,
    current_version: Option<&str>,
) -> Product {
    let mut product = Product::new(name, slug, product_type, url, &working_copy.to_string_lossy());
    product.current_version = current_version.map(str::to_owned);
    product
}

pub struct Harness {
    pub repo: LocalSvnRepository,
    pub work: TempDir,
    pub store: Arc<Store>,
    pub svn: Arc<FaultInjectingSvn>,
    pub notifications: Arc<RecordingNotifications>,
    pub pipeline: ReleasePipelineService,
    pub paths: AppPaths,
    pub transcript: Arc<SvnTranscript>,
}

impl Harness {
    /// `None` when svn/svnadmin are not installed (the test then passes as skipped).
    pub fn new() -> Option<Self> {
        let repo = LocalSvnRepository::new_or_skip()?;
        let work = TempDir::new();
        let store = Arc::new(Store::in_memory().unwrap());
        let paths = AppPaths::new(work.path("data"));
        paths.create_all().unwrap();
        let transcript = Arc::new(SvnTranscript::new(paths.transcripts()));
        let base = SvnService::new()
            .with_command_timeout(Duration::from_secs(60))
            .with_transcript(transcript.clone());
        let svn = Arc::new(FaultInjectingSvn::new(Arc::new(base)));
        let notifications = Arc::new(RecordingNotifications::default());
        let pipeline = Self::make_pipeline(
            &store,
            &svn,
            &paths,
            Arc::new(StubGit::new()),
            Some(notifications.clone()),
        );
        Some(Self {
            repo,
            work,
            store,
            svn,
            notifications,
            pipeline,
            paths,
            transcript,
        })
    }

    pub fn dependencies(
        store: &Arc<Store>,
        svn: Arc<dyn SvnServicing>,
        paths: &AppPaths,
        git: Arc<dyn GitServicing>,
    ) -> PipelineDependencies {
        PipelineDependencies::new(
            store.clone(),
            store.clone(),
            svn,
            git,
            Arc::new(InMemoryKeychain::new()),
            Arc::new(ReleaseStagingService::new(paths.clone())),
        )
        .with_generator("tests")
    }

    pub fn make_pipeline(
        store: &Arc<Store>,
        svn: &Arc<FaultInjectingSvn>,
        paths: &AppPaths,
        git: Arc<dyn GitServicing>,
        notifications: Option<Arc<RecordingNotifications>>,
    ) -> ReleasePipelineService {
        let mut deps = Self::dependencies(store, svn.clone(), paths, git);
        if let Some(notifications) = notifications {
            deps = deps.with_notifications(notifications);
        }
        ReleasePipelineService::new(deps)
    }

    pub fn register_plugin(
        &self,
        mode: ReleaseMode,
        automation: bool,
        initial_version: Option<&str>,
        assets: bool,
    ) -> Product {
        self.repo
            .create_plugin("sample-plugin", initial_version, "Sample Plugin")
            .unwrap();
        let mut product = product(
            "Sample Plugin",
            "sample-plugin",
            ProductType::Plugin,
            &format!("{}/sample-plugin", self.repo.url()),
            &self.work.path("wc-plugin"),
            initial_version,
        );
        product.release_mode = mode;
        product.automation_enabled = automation;
        if assets {
            self.work.write("assets/banner-772x250.png", "PNG");
            self.work.write("assets/icon.svg", "<svg/>");
            product.assets_path = Some(self.work.path("assets").to_string_lossy().into_owned());
        }
        self.store.save_product(&product).unwrap();
        product
    }

    /// `registerPlugin()` with its defaults.
    pub fn plugin(&self) -> Product {
        self.register_plugin(ReleaseMode::Assisted, false, Some("1.0.0"), false)
    }

    pub fn zip_source(&self, version: &str, stable_tag: Option<&str>, extra: &[(&str, &str)]) -> ReleaseSource {
        let src = TempDir::new();
        let plugin = fixtures::Plugin::new(version)
            .stable_tag(stable_tag)
            .extra(extra)
            .make(&src);
        let zip = self
            .work
            .path(&format!("sample-plugin-{}.zip", &Id::new().to_string()[..4]));
        zip_folder(&plugin, &zip);
        ReleaseSource::zip(zip.to_string_lossy())
    }

    pub fn zip(&self, version: &str) -> ReleaseSource {
        self.zip_source(version, None, &[])
    }

    pub fn folder_source(&self, version: &str, extra: &[(&str, &str)]) -> ReleaseSource {
        let at = format!("dev-{}/sample-plugin", &Id::new().to_string()[..6]);
        let root = fixtures::Plugin::new(version).at(&at).extra(extra).make(&self.work);
        ReleaseSource::folder(root.to_string_lossy())
    }

    pub async fn local_change_count(&self, product: &Product) -> usize {
        rd_pipeline::working_copy::local_changes(&product.working_copy_dir(), self.svn.as_ref())
            .await
            .unwrap()
            .len()
    }
}
