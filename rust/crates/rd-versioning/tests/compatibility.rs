//! Stored types read and write the Swift app's JSON (synthesized `Codable`) unchanged.

use rd_model::ProductType;
use rd_versioning::{DetectedProduct, PixelSize, VersionBumpPlan, WordPressReadme};

const SWIFT_DETECTED_THEME: &str = r#"{
  "headers": {"Template": "twentytwentyfive", "Theme Name": "Lumen", "Version": "1.1.0"},
  "mainFile": "style.css",
  "name": "Lumen",
  "problems": [],
  "readme": {
    "byteCount": 412,
    "fields": {"stable tag": "1.1.0", "tested up to": "6.8"},
    "name": "Lumen",
    "sections": {"changelog": "= 1.1.0 =\n* Release."},
    "shortDescription": "A theme."
  },
  "readmeFile": "readme.txt",
  "screenshotFile": "screenshot.png",
  "screenshotSize": {"height": 900, "width": 1200},
  "textDomain": "lumen",
  "type": "theme",
  "version": "1.1.0"
}"#;

#[test]
fn reads_a_detected_product_stored_by_the_swift_app() {
    let detected: DetectedProduct = serde_json::from_str(SWIFT_DETECTED_THEME).unwrap();
    assert_eq!(detected.product_type, ProductType::Theme);
    assert_eq!(detected.parent_theme(), Some("twentytwentyfive"));
    assert_eq!(detected.screenshot_size, Some(PixelSize::new(1200, 900)));
    let readme = detected.readme.as_ref().unwrap();
    assert_eq!(readme.stable_tag().as_deref(), Some("1.1.0"));
    assert_eq!(readme.byte_count, Some(412));
    assert_eq!(readme.unknown_header_lines, None);
    assert_eq!(readme.changelog_entry("1.1.0").as_deref(), Some("* Release."));
}

#[test]
fn detected_product_round_trips() {
    let detected: DetectedProduct = serde_json::from_str(SWIFT_DETECTED_THEME).unwrap();
    let value = serde_json::to_value(&detected).unwrap();
    let original: serde_json::Value = serde_json::from_str(SWIFT_DETECTED_THEME).unwrap();
    assert_eq!(value, original);
    assert_eq!(serde_json::from_value::<DetectedProduct>(value).unwrap(), detected);
}

#[test]
fn absent_optionals_are_omitted() {
    let mut readme = WordPressReadme::new(None, Default::default(), Default::default());
    readme.unknown_header_lines = Some(vec!["Gallery for WordPress: fast.".into()]);
    let detected = DetectedProduct {
        readme: Some(readme),
        problems: vec!["Multiple files declare \"Plugin Name:\" (a.php, b.php).".into()],
        ..DetectedProduct::new(ProductType::Plugin)
    };
    let value = serde_json::to_value(&detected).unwrap();
    assert_eq!(
        value,
        serde_json::json!({
            "type": "plugin",
            "headers": {},
            "problems": ["Multiple files declare \"Plugin Name:\" (a.php, b.php)."],
            "readme": {"fields": {}, "sections": {}, "unknownHeaderLines": ["Gallery for WordPress: fast."]}
        })
    );
}

const SWIFT_PLAN: &str = r#"{
  "changes": [
    {"description": "Version header", "file": "sample-plugin.php", "line": 5, "newText": " * Version: 2.4.0", "oldText": " * Version: 2.3.0"},
    {"description": "Changelog entry", "file": "readme.txt", "line": 0, "newText": "= 2.4.0 - 2026-11-24 =\n* New: Lightbox", "oldText": ""}
  ],
  "contents": {"readme.txt": "…", "sample-plugin.php": "…"},
  "currentVersion": "2.3.0",
  "fileHashes": {"readme.txt": "aa", "sample-plugin.php": "bb"},
  "newVersion": "2.4.0",
  "notes": []
}"#;

#[test]
fn version_bump_plan_round_trips() {
    let plan: VersionBumpPlan = serde_json::from_str(SWIFT_PLAN).unwrap();
    assert_eq!(plan.files(), ["sample-plugin.php", "readme.txt"]);
    assert_eq!(plan.changes[0].id(), "sample-plugin.php:5");
    let value = serde_json::to_value(&plan).unwrap();
    assert_eq!(value, serde_json::from_str::<serde_json::Value>(SWIFT_PLAN).unwrap());
}
