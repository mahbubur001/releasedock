//! Ported from Tests/ReleaseDockCoreTests/Support/TestSupport.swift (`TempDir`, `Fixtures`), the
//! `makeProduct` helper in CoreTests.swift and the `writePNG` helper of ThemeChecksTests.swift;
//! only what the validation tests use.
#![allow(dead_code, clippy::unwrap_used)]

use rd_inspection::{FolderInspecting, FolderInspectionService, InspectionOptions};
use rd_model::{PackageInspection, Product, ProductType, ReleaseMode};
use rd_support::hashing::Crc32;
use rd_versioning::{DetectedProduct, ProductDetector};
use std::fs;
use std::path::{Path, PathBuf};

/// Temporary directory removed on drop. The path has symlinks resolved, like the Swift helper.
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

    pub fn write(&self, relative: &str, contents: &str) -> PathBuf {
        let file = self.path(relative);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, contents).unwrap();
        file
    }
}

/// Swift's `makeProduct(type:slug:url:workingCopy:mode:automation:)` from `CoreTests.swift`.
pub fn make_product_with(product_type: ProductType, slug: &str, mode: ReleaseMode, automation: bool) -> Product {
    let (name, host) = match product_type {
        ProductType::Plugin => ("Sample Plugin", "plugins"),
        ProductType::Theme => ("Sample Theme", "themes"),
    };
    let mut product = Product::new(
        name,
        slug,
        product_type,
        &format!("https://{host}.svn.wordpress.org/{slug}"),
        "/tmp/wc",
    );
    product.release_mode = mode;
    product.automation_enabled = automation;
    product
}

/// `makeProduct()` with every default.
pub fn make_product() -> Product {
    make_product_with(ProductType::Plugin, "sample-plugin", ReleaseMode::Assisted, false)
}

/// `makeProduct(type:slug:)`.
pub fn make_product_of(product_type: ProductType, slug: &str) -> Product {
    make_product_with(product_type, slug, ReleaseMode::Assisted, false)
}

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

/// `Fixtures.pluginMainFile(version:)` with the default name and text domain.
pub fn default_plugin_main_file(version: &str) -> String {
    plugin_main_file("Sample Plugin", version, "sample-plugin")
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

/// `Fixtures.makePlugin(in:at:version:stableTag:name:extraFiles:)` with the default name and
/// location (`sample-plugin`). Returns the plugin root.
pub fn make_plugin(dir: &TempDir, version: &str, stable_tag: Option<&str>, extra_files: &[(&str, &str)]) -> PathBuf {
    let relative = "sample-plugin";
    let name = "Sample Plugin";
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
        "/*
Theme Name: {name}
Author: Test Author
Description: A sample theme.
Version: {version}
Requires at least: 6.0
Tested up to: 6.8
Requires PHP: 7.4
License: GNU General Public License v2 or later
License URI: http://www.gnu.org/licenses/gpl-2.0.html
Text Domain: sample-theme
Tags: blog, one-column
*/"
    )
}

/// `Fixtures.makeTheme(in:at:version:name:)` at `sample-theme`. Returns the theme root.
pub fn make_theme(dir: &TempDir, version: &str, name: &str) -> PathBuf {
    let relative = "sample-theme";
    dir.write(&format!("{relative}/style.css"), &theme_style(name, version));
    dir.write(&format!("{relative}/index.php"), "<?php get_header(); ?>\n");
    dir.write(&format!("{relative}/functions.php"), &format!("<?php // v{version}\n"));
    dir.write(&format!("{relative}/screenshot.png"), "PNG-DATA");
    dir.write(
        &format!("{relative}/readme.txt"),
        &format!(
            "=== {name} ===
Contributors: testauthor
Requires at least: 6.0
Tested up to: 6.8
Requires PHP: 7.4
License: GPLv2 or later
License URI: http://www.gnu.org/licenses/gpl-2.0.html

== Changelog ==

= {version} =
* Release."
        ),
    );
    dir.path(relative)
}

/// `FolderInspectionService().inspect(folderAt:options: InspectionOptions())`.
pub fn inspect(root: &Path) -> PackageInspection {
    FolderInspectionService::new()
        .inspect(root, &InspectionOptions::default())
        .unwrap()
}

/// `ProductDetector().detect(at:expectedType:slug:)`.
pub fn detect(root: &Path, expected_type: Option<ProductType>, slug: Option<&str>) -> DetectedProduct {
    ProductDetector::new().detect(root, expected_type, None, slug).unwrap()
}

/// Writes a PNG of the given size (replaces the CoreGraphics `writePNG` of ThemeChecksTests).
/// Only the signature and IHDR matter for reading the size; the image data is a single
/// zlib stored block of black RGBA scanlines.
pub fn write_png(path: &Path, width: u32, height: u32) {
    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend((data.len() as u32).to_be_bytes());
        let mut crc = Crc32::new();
        crc.update(kind);
        crc.update(data);
        out.extend(kind);
        out.extend(data);
        out.extend(crc.checksum().to_be_bytes());
    }
    let mut ihdr = Vec::new();
    ihdr.extend(width.to_be_bytes());
    ihdr.extend(height.to_be_bytes());
    ihdr.extend([8, 6, 0, 0, 0]); // 8-bit RGBA, no interlace
    let raw: Vec<u8> = (0..height)
        .flat_map(|_| std::iter::once(0u8).chain(std::iter::repeat_n(0u8, width as usize * 4)))
        .collect();
    let mut zlib = vec![0x78, 0x01];
    for (index, block) in raw.chunks(65_535).enumerate() {
        let last = (index + 1) * 65_535 >= raw.len();
        zlib.push(u8::from(last));
        zlib.extend((block.len() as u16).to_le_bytes());
        zlib.extend((!(block.len() as u16)).to_le_bytes());
        zlib.extend(block);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for byte in &raw {
        a = (a + u32::from(*byte)) % 65_521;
        b = (b + a) % 65_521;
    }
    zlib.extend(((b << 16) | a).to_be_bytes());
    let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    chunk(&mut png, b"IHDR", &ihdr);
    chunk(&mut png, b"IDAT", &zlib);
    chunk(&mut png, b"IEND", &[]);
    fs::write(path, png).unwrap();
}
