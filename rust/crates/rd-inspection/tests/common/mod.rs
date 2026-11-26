//! Test helpers ported from `Tests/ReleaseDockCoreTests/Support/TestSupport.swift`.
//!
//! `/usr/bin/zip` is replaced by [`zip_folder`], which writes deflate archives itself so
//! the tests run on every platform.
#![allow(dead_code, clippy::unwrap_used)]

use flate2::Compression;
use flate2::write::{DeflateEncoder, ZlibEncoder};
use rd_support::hashing::Crc32;
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Temporary directory removed on drop. The path has its symlinks resolved, like the
/// Swift helper (`/var` → `/private/var` on macOS).
pub struct TempDir {
    _dir: tempfile::TempDir,
    pub url: PathBuf,
}

impl TempDir {
    pub fn new() -> Self {
        let dir = tempfile::Builder::new().prefix("ReleaseDockTests-").tempdir().unwrap();
        let url = fs::canonicalize(dir.path()).unwrap();
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

pub mod fixtures {
    use super::TempDir;
    use std::path::PathBuf;

    pub fn plugin_main_file(name: &str, version: &str, text_domain: &str) -> String {
        format!(
            "<?php
/**
 * Plugin Name: {name}
 * Description: A sample plugin.
 * Version: {version}
 * Requires at least: 6.0
 * Requires PHP: 7.4
 * Author: Test Author
 * License: GPL-2.0-or-later
 * Text Domain: {text_domain}
 */
defined( 'ABSPATH' ) || exit;"
        )
    }

    pub fn plugin_readme(name: &str, stable_tag: &str, version: Option<&str>) -> String {
        let version = version.unwrap_or(stable_tag);
        format!(
            "=== {name} ===
Contributors: testauthor
Tags: testing, release
Requires at least: 6.0
Tested up to: 6.8
Requires PHP: 7.4
Stable tag: {stable_tag}
License: GPLv2 or later
License URI: https://www.gnu.org/licenses/gpl-2.0.html

A sample plugin.

== Description ==

Does sample things.

== Changelog ==

= {version} =
* Release.

= 0.9.0 =
* Earlier."
        )
    }

    /// `Fixtures.makePlugin(in:at:version:stableTag:name:extraFiles:)` with its defaults.
    pub struct Plugin<'a> {
        pub relative: &'a str,
        pub version: &'a str,
        pub stable_tag: Option<&'a str>,
        pub name: &'a str,
        pub extra_files: Vec<(&'a str, String)>,
    }

    impl<'a> Plugin<'a> {
        pub fn new(version: &'a str) -> Self {
            Self {
                relative: "sample-plugin",
                version,
                stable_tag: None,
                name: "Sample Plugin",
                extra_files: Vec::new(),
            }
        }

        pub fn extra(mut self, files: &[(&'a str, &str)]) -> Self {
            self.extra_files
                .extend(files.iter().map(|(p, c)| (*p, (*c).to_owned())));
            self
        }

        /// Writes the plugin into `dir/relative` and returns its root.
        pub fn make(self, dir: &TempDir) -> PathBuf {
            let r = self.relative;
            let version = self.version;
            dir.write(
                &format!("{r}/sample-plugin.php"),
                plugin_main_file(self.name, version, "sample-plugin"),
            );
            dir.write(
                &format!("{r}/readme.txt"),
                plugin_readme(self.name, self.stable_tag.unwrap_or(version), Some(version)),
            );
            dir.write(&format!("{r}/includes/class-sample.php"), "<?php\nclass Sample {}\n");
            dir.write(
                &format!("{r}/assets/js/app.js"),
                format!("console.log('v{version}');\n"),
            );
            for (path, contents) in self.extra_files {
                dir.write(&format!("{r}/{path}"), contents);
            }
            dir.path(r)
        }
    }

    /// `Fixtures.makePlugin(in: dir, version:)`.
    pub fn make_plugin(dir: &TempDir, version: &str) -> PathBuf {
        Plugin::new(version).make(dir)
    }
}

fn le16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn le32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

/// Builds ZIP archives byte by byte (stored entries only) so tests can produce
/// malicious structures that normal tools refuse to create.
#[derive(Clone, Default)]
pub struct TestZipBuilder {
    pub entries: Vec<Entry>,
}

#[derive(Clone)]
pub struct Entry {
    pub name: String,
    pub data: Vec<u8>,
    pub unix_mode: u32,
    pub flags: u16,
    pub local_name: Option<String>,
}

impl Entry {
    pub fn new(name: &str, data: &[u8]) -> Self {
        Self {
            name: name.to_owned(),
            data: data.to_vec(),
            unix_mode: 0o100644,
            flags: 0x0800,
            local_name: None,
        }
    }
}

impl TestZipBuilder {
    pub fn add(&mut self, name: &str, contents: &str) {
        self.add_with_mode(name, contents, 0o100644);
    }

    pub fn add_with_mode(&mut self, name: &str, contents: &str, mode: u32) {
        let mut entry = Entry::new(name, contents.as_bytes());
        entry.unix_mode = mode;
        self.entries.push(entry);
    }

    pub fn add_directory(&mut self, name: &str) {
        let name = if name.ends_with('/') {
            name.to_owned()
        } else {
            format!("{name}/")
        };
        let mut entry = Entry::new(&name, b"");
        entry.unix_mode = 0o040755;
        self.entries.push(entry);
    }

    pub fn add_symlink(&mut self, name: &str, target: &str) {
        let mut entry = Entry::new(name, target.as_bytes());
        entry.unix_mode = 0o120777;
        self.entries.push(entry);
    }

    pub fn build(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut central = Vec::new();
        for entry in &self.entries {
            let offset = out.len() as u32;
            let name = entry.name.as_bytes();
            let local_name = entry.local_name.as_deref().unwrap_or(&entry.name).as_bytes();
            let crc = Crc32::of(&entry.data);
            let size = entry.data.len() as u32;
            le32(&mut out, 0x0403_4B50);
            for v in [20, entry.flags, 0, 0, 0] {
                le16(&mut out, v);
            }
            le32(&mut out, crc);
            le32(&mut out, size);
            le32(&mut out, size);
            le16(&mut out, local_name.len() as u16);
            le16(&mut out, 0);
            out.extend_from_slice(local_name);
            out.extend_from_slice(&entry.data);

            le32(&mut central, 0x0201_4B50);
            for v in [0x031E, 20, entry.flags, 0, 0, 0] {
                le16(&mut central, v);
            }
            le32(&mut central, crc);
            le32(&mut central, size);
            le32(&mut central, size);
            for v in [name.len() as u16, 0, 0, 0, 0] {
                le16(&mut central, v);
            }
            le32(&mut central, entry.unix_mode << 16);
            le32(&mut central, offset);
            central.extend_from_slice(name);
        }
        finish(out, central, self.entries.len())
    }

    pub fn write(&self, path: &Path) {
        fs::write(path, self.build()).unwrap();
    }
}

fn finish(mut out: Vec<u8>, central: Vec<u8>, count: usize) -> Vec<u8> {
    let cd_offset = out.len() as u32;
    out.extend_from_slice(&central);
    le32(&mut out, 0x0605_4B50);
    for v in [0, 0, count as u16, count as u16] {
        le16(&mut out, v);
    }
    le32(&mut out, central.len() as u32);
    le32(&mut out, cd_offset);
    le16(&mut out, 0);
    out
}

/// Zips `folder` (including the folder itself as the archive root), like
/// `zip -q -r -y`: directory entries, deflated files (empty files stored), symlinks
/// stored as links, and Info-ZIP style extra fields that differ between the local and
/// central headers.
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
        // Local header with "UT" (timestamp) and "ux" (uid/gid) extra fields.
        le32(&mut out, 0x0403_4B50);
        for v in [20, 0, method, 0, 0] {
            le16(&mut out, v);
        }
        le32(&mut out, crc);
        le32(&mut out, stored.len() as u32);
        le32(&mut out, data.len() as u32);
        le16(&mut out, name.len() as u16);
        le16(&mut out, 9 + 15);
        out.extend_from_slice(name.as_bytes());
        le16(&mut out, 0x5455);
        le16(&mut out, 5);
        out.push(1);
        le32(&mut out, 1_700_000_000);
        le16(&mut out, 0x7875);
        le16(&mut out, 11);
        out.extend_from_slice(&[1, 4, 0xF5, 1, 0, 0, 4, 0x14, 0, 0, 0]);
        out.extend_from_slice(&stored);

        le32(&mut central, 0x0201_4B50);
        for v in [0x031E, 20, 0, method, 0, 0] {
            le16(&mut central, v);
        }
        le32(&mut central, crc);
        le32(&mut central, stored.len() as u32);
        le32(&mut central, data.len() as u32);
        for v in [name.len() as u16, 9, 0, 0, 0] {
            le16(&mut central, v);
        }
        le32(&mut central, mode << 16);
        le32(&mut central, offset);
        central.extend_from_slice(name.as_bytes());
        le16(&mut central, 0x5455);
        le16(&mut central, 5);
        central.push(1);
        le32(&mut central, 1_700_000_000);
    }
    fs::write(destination, finish(out, central, items.len())).unwrap();
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

/// Writes a valid RGBA PNG of the given size (replaces CoreGraphics + ImageIO).
pub fn write_png(path: &Path, width: u32, height: u32) {
    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend_from_slice(data);
        out.extend_from_slice(&body);
        out.extend_from_slice(&Crc32::of(&body).to_be_bytes());
    }
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut header = Vec::new();
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    header.extend_from_slice(&[8, 6, 0, 0, 0]);
    let row = vec![0u8; 1 + width as usize * 4];
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
    for _ in 0..height {
        encoder.write_all(&row).unwrap();
    }
    let pixels = encoder.finish().unwrap();
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    chunk(&mut png, b"IHDR", &header);
    chunk(&mut png, b"IDAT", &pixels);
    chunk(&mut png, b"IEND", &[]);
    fs::write(path, png).unwrap();
}

/// Every path beneath `root`, sorted (`FileManager.subpathsOfDirectory`).
pub fn subpaths(root: &Path) -> Vec<String> {
    let mut items = BTreeMap::new();
    collect(root, "", &mut items);
    items.into_keys().filter(|k| !k.is_empty()).collect()
}
