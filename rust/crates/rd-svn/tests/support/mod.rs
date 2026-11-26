//! Port of the SVN-related parts of `Tests/ReleaseDockCoreTests/Support/TestSupport.swift`
//! and the `RecordingRunner` from `CoreTests.swift`.
#![allow(dead_code)]

use async_trait::async_trait;
use rd_svn::{ProcessRequest, ProcessResult, ProcessRunnerError, ProcessRunning, SvnService};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::Duration;

/// Temporary directory removed on drop, with symlinks resolved (macOS `/var` → `/private/var`)
/// so paths match what svn reports.
pub struct TempDir {
    _dir: tempfile::TempDir,
    pub path: PathBuf,
}

impl TempDir {
    pub fn new() -> Self {
        let dir = tempfile::Builder::new()
            .prefix("ReleaseDockTests-")
            .tempdir()
            .expect("temp dir");
        let path = dir.path().canonicalize().expect("canonical temp dir");
        Self { _dir: dir, path }
    }

    pub fn path(&self, relative: &str) -> PathBuf {
        self.path.join(relative)
    }

    pub fn write(&self, relative: &str, contents: &str) -> PathBuf {
        let file = self.path(relative);
        std::fs::create_dir_all(file.parent().expect("parent")).expect("mkdir");
        std::fs::write(&file, contents).expect("write");
        file
    }

    pub fn mkdir(&self, relative: &str) {
        std::fs::create_dir_all(self.path(relative)).expect("mkdir");
    }
}

/// Records requests and answers like `svn` would.
#[derive(Default)]
pub struct RecordingRunner {
    requests: Mutex<Vec<ProcessRequest>>,
}

impl RecordingRunner {
    pub fn requests(&self) -> Vec<ProcessRequest> {
        self.requests.lock().expect("requests lock").clone()
    }
}

#[async_trait]
impl ProcessRunning for RecordingRunner {
    async fn run(&self, request: ProcessRequest) -> Result<ProcessResult, ProcessRunnerError> {
        self.requests.lock().expect("requests lock").push(request.clone());
        let output = if request.arguments.first().map(String::as_str) == Some("commit") {
            "Committed revision 42.\n"
        } else {
            "1.14.5\n"
        };
        Ok(ProcessResult {
            exit_code: 0,
            standard_output: output.as_bytes().to_vec(),
            standard_error: Vec::new(),
            duration: Duration::ZERO,
            display_command: request.display_command(),
        })
    }
}

/// An existing executable file to hand to `SvnService::with_executable` when the runner is
/// fake (Swift uses `/usr/bin/true`; the test binary itself exists on every platform).
pub fn stand_in_executable() -> PathBuf {
    std::env::current_exe().expect("current exe")
}

pub mod fixtures {
    use super::TempDir;
    use std::collections::BTreeMap;
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

    /// Writes a plugin into `dir/relative` and returns its root.
    pub fn make_plugin(
        dir: &TempDir,
        relative: &str,
        version: &str,
        stable_tag: Option<&str>,
        name: &str,
        extra_files: &BTreeMap<&str, &str>,
    ) -> PathBuf {
        dir.write(
            &format!("{relative}/sample-plugin.php"),
            &plugin_main_file(name, version, "sample-plugin"),
        );
        dir.write(
            &format!("{relative}/readme.txt"),
            &plugin_readme(name, stable_tag.unwrap_or(version), Some(version)),
        );
        dir.write(
            &format!("{relative}/includes/class-sample.php"),
            "<?php\nclass Sample {}\n",
        );
        dir.write(
            &format!("{relative}/assets/js/app.js"),
            &format!("console.log('v{version}');\n"),
        );
        for (path, contents) in extra_files {
            dir.write(&format!("{relative}/{path}"), contents);
        }
        dir.path(relative)
    }

    pub fn theme_style(name: &str, version: &str) -> String {
        format!(
            "/*\nTheme Name: {name}\nAuthor: Test Author\nDescription: A sample theme.\nVersion: {version}\nRequires at least: 6.0\nTested up to: 6.8\nRequires PHP: 7.4\nLicense: GNU General Public License v2 or later\nLicense URI: http://www.gnu.org/licenses/gpl-2.0.html\nText Domain: sample-theme\nTags: blog, one-column\n*/"
        )
    }

    pub fn make_theme(dir: &TempDir, relative: &str, version: &str, name: &str) -> PathBuf {
        dir.write(&format!("{relative}/style.css"), &theme_style(name, version));
        dir.write(&format!("{relative}/index.php"), "<?php get_header(); ?>\n");
        dir.write(&format!("{relative}/functions.php"), &format!("<?php // v{version}\n"));
        dir.write(&format!("{relative}/screenshot.png"), "PNG-DATA");
        dir.write(
            &format!("{relative}/readme.txt"),
            &format!(
                "=== {name} ===\nContributors: testauthor\nRequires at least: 6.0\nTested up to: 6.8\nRequires PHP: 7.4\nLicense: GPLv2 or later\nLicense URI: http://www.gnu.org/licenses/gpl-2.0.html\n\n== Changelog ==\n\n= {version} =\n* Release."
            ),
        );
        dir.path(relative)
    }
}

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

    pub fn svnadmin_executable() -> Option<PathBuf> {
        find_tool("svnadmin")
    }

    pub fn is_available() -> bool {
        Self::svn_executable().is_some() && Self::svnadmin_executable().is_some()
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
        std::fs::create_dir_all(root.parent().ok_or("no parent")?).map_err(|e| e.to_string())?;
        Self::create(root)?;
        Ok(Self {
            dir: TempDir::new(),
            root: root.to_path_buf(),
            svn: Self::svn_executable().ok_or("svn not installed")?,
        })
    }

    fn create(root: &Path) -> Result<(), String> {
        let admin = Self::svnadmin_executable().ok_or("svnadmin not installed")?;
        Self::run_tool(&admin, &["create".into(), root.to_string_lossy().into_owned()], None)?;
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

    pub fn run(&self, args: &[String]) -> Result<String, String> {
        self.run_in(args, None)
    }

    pub fn run_in(&self, args: &[String], cwd: Option<&Path>) -> Result<String, String> {
        let mut all = args.to_vec();
        all.push("--non-interactive".into());
        Self::run_tool(&self.svn, &all, cwd)
    }

    pub fn run_tool(exe: &Path, args: &[String], cwd: Option<&Path>) -> Result<String, String> {
        let mut command = Command::new(exe);
        command
            .args(args)
            .env_clear()
            .envs(ProcessRequest::default_environment(
                &rd_svn::process_runner::home_directory(),
            ))
            .stdin(Stdio::null());
        if let Some(cwd) = cwd {
            command.current_dir(cwd);
        }
        let output = command.output().map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).into_owned());
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    /// Creates `/<slug>/{trunk,tags,assets}` and optionally publishes an initial version.
    pub fn create_plugin(&self, slug: &str, initial_version: Option<&str>, name: &str) -> Result<(), String> {
        let url = self.url();
        self.run(&strings(&[
            "mkdir",
            "--parents",
            "-m",
            &format!("Create {slug}"),
            &format!("{url}/{slug}/trunk"),
            &format!("{url}/{slug}/tags"),
            &format!("{url}/{slug}/assets"),
        ]))?;
        let Some(initial_version) = initial_version else {
            return Ok(());
        };
        let src = TempDir::new();
        let plugin = fixtures::make_plugin(&src, "p", initial_version, None, name, &Default::default());
        let wc = self.dir.path(&format!("seed-{}", rd_model::Id::new()));
        self.run(&strings(&["checkout", &format!("{url}/{slug}"), &wc.to_string_lossy()]))?;
        let trunk = wc.join("trunk");
        copy_dir_contents(&plugin, &trunk).map_err(|e| e.to_string())?;
        self.run(&strings(&["add", "--force", &trunk.to_string_lossy()]))?;
        self.run(&strings(&[
            "copy",
            &trunk.to_string_lossy(),
            &wc.join(format!("tags/{initial_version}")).to_string_lossy(),
        ]))?;
        self.run(&strings(&[
            "commit",
            "-m",
            &format!("Initial {initial_version}"),
            &wc.to_string_lossy(),
        ]))?;
        Ok(())
    }

    pub fn create_theme(&self, slug: &str, initial_version: Option<&str>, name: &str) -> Result<(), String> {
        let url = self.url();
        self.run(&strings(&[
            "mkdir",
            "--parents",
            "-m",
            &format!("Create {slug}"),
            &format!("{url}/{slug}"),
        ]))?;
        let Some(initial_version) = initial_version else {
            return Ok(());
        };
        let src = TempDir::new();
        let theme = fixtures::make_theme(&src, "t", initial_version, name);
        self.run(&strings(&[
            "import",
            "-m",
            &format!("Initial {initial_version}"),
            &theme.to_string_lossy(),
            &format!("{url}/{slug}/{initial_version}"),
        ]))?;
        Ok(())
    }
}

pub fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|v| (*v).to_owned()).collect()
}

fn copy_dir_contents(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_contents(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}
