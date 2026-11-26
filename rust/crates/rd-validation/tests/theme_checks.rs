//! Ported from ThemeChecksTests.swift. `testDetectorReadsScreenshotSize` only tests the detector
//! and is in rd-versioning (`tests/detector.rs`).

mod common;

use common::{TempDir, detect, inspect, make_product, make_theme, theme_style, write_png};
use rd_model::ProductType;
use rd_validation::{ValidationContext, ValidationService, ValidationServicing};

/// Swift `warnings(name:screenshot:extra:description:)`.
struct Theme {
    name: &'static str,
    screenshot: Option<(u32, u32)>,
    extra: Vec<(&'static str, &'static str)>,
    description: Option<&'static str>,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            name: "Lumen",
            screenshot: Some((1200, 900)),
            extra: Vec::new(),
            description: None,
        }
    }
}

impl Theme {
    fn warnings(self) -> Vec<String> {
        let dir = TempDir::new();
        let root = make_theme(&dir, "1.1.0", self.name);
        if let Some((width, height)) = self.screenshot {
            write_png(&root.join("screenshot.png"), width, height);
        }
        if let Some(description) = self.description {
            dir.write(
                "sample-theme/style.css",
                &theme_style(self.name, "1.1.0").replace("A sample theme.", description),
            );
        }
        for (path, contents) in &self.extra {
            dir.write(&format!("sample-theme/{path}"), contents);
        }
        let mut product = make_product();
        product.product_type = ProductType::Theme;
        product.expected_header_name = self.name.into();
        let inspection = inspect(&root);
        let detected = detect(&root, Some(ProductType::Theme), None);
        let report =
            ValidationService::new().validate(&ValidationContext::new(product, inspection, Some(detected), None));
        report.warnings().iter().map(|r| r.message.clone()).collect()
    }
}

#[test]
fn correct_screenshot_and_name_have_no_theme_warnings() {
    let found = Theme::default().warnings();
    assert!(
        !found.iter().any(|w| w.contains("screenshot")
            || w.contains("Theme names")
            || w.contains("Spell")
            || w.contains("Resources")),
        "{found:?}"
    );
}

#[test]
fn screenshot_size_and_ratio() {
    let large = Theme {
        screenshot: Some((1600, 1200)),
        ..Theme::default()
    }
    .warnings();
    assert!(large.contains(&"screenshot.png is 1600×1200; theme review allows at most 1200×900".to_owned()));
    let wide = Theme {
        screenshot: Some((1200, 600)),
        ..Theme::default()
    }
    .warnings();
    assert!(
        wide.contains(&"screenshot.png is 1200×600, not 4:3 as theme review requires".to_owned()),
        "{wide:?}"
    );
    assert!(!wide.iter().any(|w| w.contains("at most")));
}

#[test]
fn reserved_names_and_spelling() {
    for name in ["Sample Theme", "WordPress Starter", "Twentyish"] {
        assert!(
            Theme {
                name,
                ..Theme::default()
            }
            .warnings()
            .iter()
            .any(|w| w.contains("Theme names must not use")),
            "{name}"
        );
    }
    assert!(
        !Theme {
            name: "Themely",
            ..Theme::default()
        }
        .warnings()
        .iter()
        .any(|w| w.contains("Theme names must not use")),
        "only whole words"
    );
    assert!(
        Theme {
            description: Some("A clean Wordpress blog theme."),
            ..Theme::default()
        }
        .warnings()
        .iter()
        .any(|w| w.starts_with("Spell \"WordPress\""))
    );
    assert!(
        !Theme {
            description: Some("Works on wordpress.org and WordPress."),
            ..Theme::default()
        }
        .warnings()
        .iter()
        .any(|w| w.starts_with("Spell"))
    );
}

#[test]
fn bundled_resources_need_a_resources_section() {
    let missing = Theme {
        extra: vec![("assets/images/hero.jpg", "img"), ("assets/fonts/inter.woff2", "font")],
        ..Theme::default()
    }
    .warnings();
    assert!(
        missing.iter().any(|w| w.contains("bundles 2 image or font file(s)")),
        "{missing:?}"
    );
}
