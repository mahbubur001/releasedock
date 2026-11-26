//! Ported from Tests/ReleaseDockCoreTests/Support/TestSupport.swift (TempDir, Fixtures) and
//! the `makeProduct` helper in CoreTests.swift; only what the store tests use.
#![allow(dead_code, clippy::unwrap_used)]

use rd_model::{Product, ProductType, ReleaseMode};
use std::fs;
use std::path::PathBuf;

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

    pub fn mkdir(&self, relative: &str) -> PathBuf {
        let dir = self.path(relative);
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}

/// Swift's `makeProduct(type:slug:url:workingCopy:mode:automation:)` from `CoreTests.swift`.
pub fn make_product(
    product_type: ProductType,
    slug: &str,
    url: Option<&str>,
    working_copy: &str,
    mode: ReleaseMode,
    automation: bool,
) -> Product {
    let (name, host) = match product_type {
        ProductType::Plugin => ("Sample Plugin", "plugins"),
        ProductType::Theme => ("Sample Theme", "themes"),
    };
    let url = url.map_or_else(|| format!("https://{host}.svn.wordpress.org/{slug}"), str::to_owned);
    let mut product = Product::new(name, slug, product_type, &url, working_copy);
    product.release_mode = mode;
    product.automation_enabled = automation;
    product
}

/// `makeProduct()` with every default.
pub fn default_product() -> Product {
    product_with_working_copy("/tmp/wc")
}

/// `makeProduct(workingCopy:)`.
pub fn product_with_working_copy(working_copy: &str) -> Product {
    make_product(
        ProductType::Plugin,
        "sample-plugin",
        None,
        working_copy,
        ReleaseMode::Assisted,
        false,
    )
}

fn plugin_main_file(name: &str, version: &str) -> String {
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
 * Text Domain: sample-plugin
 */
defined( 'ABSPATH' ) || exit;"
    )
}

fn plugin_readme(name: &str, version: &str) -> String {
    format!(
        "=== {name} ===
Contributors: testauthor
Tags: testing, release
Requires at least: 6.0
Tested up to: 6.8
Requires PHP: 7.4
Stable tag: {version}
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

/// `Fixtures.makePlugin(in:version:)`: a plugin in `dir/sample-plugin`.
pub fn make_plugin(dir: &TempDir, version: &str) -> PathBuf {
    let name = "Sample Plugin";
    dir.write("sample-plugin/sample-plugin.php", &plugin_main_file(name, version));
    dir.write("sample-plugin/readme.txt", &plugin_readme(name, version));
    dir.write("sample-plugin/includes/class-sample.php", "<?php\nclass Sample {}\n");
    dir.write(
        "sample-plugin/assets/js/app.js",
        &format!("console.log('v{version}');\n"),
    );
    dir.path("sample-plugin")
}
