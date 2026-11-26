//! Ported from AssetRulesTests.swift.

mod common;

use common::{TempDir, write_png};
use rd_inspection::{AssetFile, AssetInspector, AssetsReport, asset_rules};

fn png(dir: &TempDir, path: &str, width: u32, height: u32) {
    write_png(&dir.path(path), width, height);
}

fn messages(dir: &TempDir) -> Vec<String> {
    asset_rules::check(&AssetInspector.inspect(&dir.url).expect("inspect assets"))
        .into_iter()
        .map(|r| r.message)
        .collect()
}

#[test]
fn valid_assets_pass() {
    let dir = TempDir::new();
    png(&dir, "banner-772x250.png", 772, 250);
    png(&dir, "banner-1544x500.png", 1544, 500);
    png(&dir, "banner-772x250-rtl.png", 772, 250);
    png(&dir, "icon-128x128.png", 128, 128);
    dir.write("icon.svg", "<svg/>");
    png(&dir, "screenshot-1.png", 40, 30);
    dir.write("screenshot-2-de_DE.jpg", "not decoded");
    dir.write("blueprints/blueprint.json", r#"{"steps":[{"step":"login"}]}"#);
    dir.write(".DS_Store", "x");
    let report = AssetInspector.inspect(&dir.url).unwrap();
    assert_eq!(
        report
            .files
            .iter()
            .find(|f| f.path == "banner-1544x500.png")
            .and_then(|f| f.width),
        Some(1544)
    );
    assert!(!report.files.iter().any(|f| f.path == ".DS_Store"));
    assert_eq!(
        messages(&dir),
        ["8 asset(s) follow WordPress.org names and sizes, blueprint.json is valid"]
    );
}

#[test]
fn wrong_dimensions_and_unknown_names() {
    let dir = TempDir::new();
    png(&dir, "banner-772x250.png", 800, 300);
    png(&dir, "Screenshot-1.png", 10, 10);
    png(&dir, "banner.png", 10, 10);
    dir.write("icon-128x128.jpeg", "x");
    let found = messages(&dir);
    assert!(
        found.contains(&"Banner is 800×300 but its name says 772×250".to_owned()),
        "{found:?}"
    );
    assert!(found.contains(&"Asset names must be lowercase; WordPress.org ignores Screenshot-1.png".to_owned()));
    assert!(found.contains(&"WordPress.org does not use banner.png".to_owned()));
    assert!(
        found.contains(&"WordPress.org does not use icon-128x128.jpeg".to_owned()),
        "only .jpg is listed in the handbook"
    );
}

#[test]
fn retina_banner_alone_and_svg_without_fallback() {
    let dir = TempDir::new();
    png(&dir, "banner-1544x500-es.png", 1544, 500);
    dir.write("icon.svg", "<svg/>");
    assert_eq!(
        messages(&dir),
        [
            "The 1544×500 banner is only used together with a 772×250 banner",
            "icon.svg needs a PNG or JPG icon as a fallback",
        ]
    );
}

#[test]
fn oversized_icon_and_invalid_blueprint() {
    let dir = TempDir::new();
    dir.write("icon-256x256.gif", "x".repeat((1 << 20) + 1));
    dir.write("blueprints/blueprint.json", "{ steps: [] }");
    let found = messages(&dir);
    assert!(
        found.contains(&"Icon is over 1 MB, the WordPress.org limit".to_owned()),
        "{found:?}"
    );
    assert!(found.contains(&"blueprint.json is not valid JSON; Live Preview will not work".to_owned()));
    let array = TempDir::new();
    array.write("blueprints/blueprint.json", "[]");
    assert_eq!(
        messages(&array),
        ["blueprint.json must contain a JSON object; Live Preview will not work"]
    );
}

/// Not in Swift: `AssetsReport` is stored in release records, so its JSON must match
/// Swift's `Codable` output (camelCase keys, absent optionals omitted).
#[test]
fn assets_report_json_round_trip() {
    let json = r#"{"files":[{"path":"banner-772x250.png","size":1024,"width":772,"height":250},{"path":"icon.svg","size":6}],"blueprintProblem":"blueprint.json is not valid JSON"}"#;
    let report: AssetsReport = serde_json::from_str(json).unwrap();
    assert_eq!(
        report.files[1],
        AssetFile {
            path: "icon.svg".into(),
            size: 6,
            width: None,
            height: None
        }
    );
    assert_eq!(serde_json::to_string(&report).unwrap(), json);
    let empty = AssetsReport::new(vec![]);
    assert_eq!(serde_json::to_string(&empty).unwrap(), r#"{"files":[]}"#);
}
