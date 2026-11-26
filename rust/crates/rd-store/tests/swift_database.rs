//! The Rust store must open a database written by the Swift app unchanged. The database here
//! is built with raw SQL exactly as Swift's `Migrator` and `Store` write it: the version 1
//! schema copied from Migrations.swift, `user_version = 1`, compact JSON with sorted keys and
//! escaped slashes in `data`, seconds since 1970 in the REAL key columns.
#![allow(clippy::unwrap_used)]

mod common;

use common::TempDir;
use rd_model::{Id, Product, ProductType, SwiftDate};
use rd_records::{ReleaseRecord, ReleaseState, ReleaseStateMachine};
use rd_store::*;
use rusqlite::{Connection, params};
use serde_json::Value;

/// `CREATE TABLE …` statements of version 1, verbatim from Migrations.swift (after Swift strips
/// the multi-line literal's indentation).
const SWIFT_SCHEMA: &str = r#"
CREATE TABLE products (
    id TEXT PRIMARY KEY NOT NULL,
    type TEXT NOT NULL,
    slug TEXT NOT NULL,
    data BLOB NOT NULL,
    created_at REAL NOT NULL,
    updated_at REAL NOT NULL
);
CREATE UNIQUE INDEX products_type_slug ON products(type, slug);

CREATE TABLE releases (
    id TEXT PRIMARY KEY NOT NULL,
    product_id TEXT NOT NULL REFERENCES products(id) ON DELETE CASCADE,
    version TEXT,
    state TEXT NOT NULL,
    data BLOB NOT NULL,
    created_at REAL NOT NULL,
    updated_at REAL NOT NULL
);
CREATE INDEX releases_product ON releases(product_id, created_at);
CREATE INDEX releases_state ON releases(state);

CREATE TABLE release_events (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    release_id TEXT NOT NULL REFERENCES releases(id) ON DELETE CASCADE,
    from_state TEXT,
    to_state TEXT NOT NULL,
    note TEXT,
    at REAL NOT NULL
);
CREATE INDEX release_events_release ON release_events(release_id, seq);

CREATE TABLE settings (
    key TEXT PRIMARY KEY NOT NULL,
    value BLOB NOT NULL
);"#;

/// A product as the Swift app stores it (the sample in rd-model's compatibility test), in
/// `JSONEncoder` `.sortedKeys` form.
const SWIFT_PRODUCT: &str = r#"{"automationEnabled":false,"createdAt":817029777.884127,"currentVersion":"1.0.24","expectedHeaderName":"Sample Plugin","id":"EE90AEAA-DAB2-45F4-805D-58DE936B984C","lastSuccessfulRelease":{"date":817293629.069138,"releaseID":"34629C18-132D-4671-9B07-748F19721D10","revision":3822482,"version":"1.0.24"},"name":"Sample Plugin","releaseMode":"assisted","slug":"sample-plugin","svnRepositoryURL":"https:\/\/plugins.svn.wordpress.org\/sample-plugin","type":"plugin","updatedAt":817293629.069138,"validation":{"blockOnWarnings":false,"disabledChecks":["phpSyntax"],"excludePatterns":[],"packageSizeWarningMB":10,"requireChangelogEntry":true,"requireCleanGitTree":false},"workingCopyPath":"\/tmp\/svn\/sample-plugin"}"#;

/// The product of the cancelled fixture, with every non-default setting.
const SWIFT_TOOLKITS: &str = r#"{"automationEnabled":false,"createdAt":817200000.5,"expectedHeaderName":"Classified Listing Toolkits","githubRelease":{"draft":true,"enabled":true,"tagPrefix":""},"githubRepositoryURL":"https:\/\/github.com\/acme\/toolkits","id":"E741A507-7BCC-4B66-8D6D-744ED501D0A3","name":"Classified Listing Toolkits","releaseMode":"dryRun","slug":"classified-listing-toolkits","svnRepositoryURL":"https:\/\/plugins.svn.wordpress.org\/classified-listing-toolkits","svnUsername":"acme","type":"plugin","updatedAt":817200000.5,"validation":{"blockOnWarnings":true,"disabledChecks":[],"excludePatterns":["*.map"],"packageSizeWarningMB":25,"requireChangelogEntry":false,"requireCleanGitTree":true},"workingCopyPath":"\/tmp\/svn\/toolkits"}"#;

const FIXTURES: [&str; 3] = [
    include_str!("../../rd-records/tests/fixtures/release-completed.json"),
    include_str!("../../rd-records/tests/fixtures/release-failed.json"),
    include_str!("../../rd-records/tests/fixtures/release-cancelled.json"),
];

const REFERENCE_OFFSET: f64 = 978_307_200.0;

/// Swift's compact `.sortedKeys` JSON for a fixture: serde_json's `Value` keeps keys sorted
/// by bytes like Foundation; Foundation additionally escapes `/`.
fn swift_compact(json: &str) -> String {
    let value: Value = serde_json::from_str(json).unwrap();
    serde_json::to_string(&value).unwrap().replace('/', "\\/")
}

/// Writes the database the way the Swift app does and returns its path.
fn make_swift_database(dir: &TempDir) -> std::path::PathBuf {
    let path = dir.path("Database/releasedock.sqlite");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let db = Connection::open(&path).unwrap();
    db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA synchronous=FULL;")
        .unwrap();
    db.execute_batch(&format!(
        "BEGIN IMMEDIATE; {SWIFT_SCHEMA} PRAGMA user_version = 1; COMMIT;"
    ))
    .unwrap();

    for product in [SWIFT_PRODUCT, SWIFT_TOOLKITS] {
        let value: Value = serde_json::from_str(product).unwrap();
        db.execute(
            "INSERT INTO products (id, type, slug, data, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?)",
            params![
                value["id"].as_str().unwrap(),
                value["type"].as_str().unwrap(),
                value["slug"].as_str().unwrap(),
                product.as_bytes(),
                value["createdAt"].as_f64().unwrap() + REFERENCE_OFFSET,
                value["updatedAt"].as_f64().unwrap() + REFERENCE_OFFSET,
            ],
        )
        .unwrap();
    }
    for fixture in FIXTURES {
        let value: Value = serde_json::from_str(fixture).unwrap();
        db.execute(
            "INSERT INTO releases (id, product_id, version, state, data, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
            params![
                value["id"].as_str().unwrap(),
                value["productID"].as_str().unwrap(),
                value["version"].as_str(),
                value["state"].as_str().unwrap(),
                swift_compact(fixture).as_bytes(),
                value["createdAt"].as_f64().unwrap() + REFERENCE_OFFSET,
                value["updatedAt"].as_f64().unwrap() + REFERENCE_OFFSET,
            ],
        )
        .unwrap();
        for transition in value["transitions"].as_array().unwrap() {
            db.execute(
                "INSERT INTO release_events (release_id, from_state, to_state, note, at) VALUES (?, ?, ?, ?, ?)",
                params![
                    value["id"].as_str().unwrap(),
                    transition["from"].as_str(),
                    transition["to"].as_str().unwrap(),
                    transition["note"].as_str(),
                    transition["at"].as_f64().unwrap() + REFERENCE_OFFSET,
                ],
            )
            .unwrap();
        }
    }
    db.execute(
        "INSERT INTO settings (key, value) VALUES ('update.notifiedVersion', ?)",
        params![b"\"0.2.3\"".as_slice()],
    )
    .unwrap();
    db.execute(
        "INSERT INTO settings (key, value) VALUES ('app.settings', ?)",
        params![br#"{"channel":"stable","checkedAt":817298196.840931,"url":"https:\/\/example.org\/feed"}"#.as_slice()],
    )
    .unwrap();
    path
}

fn raw_row(path: &std::path::Path, sql: &str) -> (Vec<u8>, f64, f64) {
    let db = Connection::open(path).unwrap();
    db.query_row(sql, [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
}

fn schema(db: &Connection) -> Vec<(String, String, Option<String>)> {
    let mut statement = db
        .prepare("SELECT type, name, sql FROM sqlite_master ORDER BY type, name")
        .unwrap();
    statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

#[test]
fn rust_migrator_creates_the_swift_schema() {
    let dir = TempDir::new();
    let swift = make_swift_database(&dir);
    drop(Store::open(&AppPaths::new(dir.path("rust"))).unwrap());
    let rust = dir.path("rust/Database/releasedock.sqlite");
    let (swift, rust) = (Connection::open(swift).unwrap(), Connection::open(rust).unwrap());
    assert_eq!(schema(&rust), schema(&swift));
    let version = |db: &Connection| db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0)).unwrap();
    assert_eq!(version(&rust), 1);
}

#[test]
fn reads_writes_and_rereads_a_swift_database() {
    let dir = TempDir::new();
    let path = make_swift_database(&dir);
    let paths = AppPaths::new(dir.url.clone());
    let store = Store::open(&paths).unwrap();
    assert!(
        std::fs::read_dir(paths.backups()).unwrap().next().is_none(),
        "an up-to-date database is not migrated or backed up"
    );

    // Read.
    let products = store.all_products().unwrap();
    let slugs: Vec<_> = products.iter().map(|p| p.slug.as_str()).collect();
    assert_eq!(
        slugs,
        ["classified-listing-toolkits", "sample-plugin"],
        "ordered by slug"
    );
    let plugin_id = Id::parse("EE90AEAA-DAB2-45F4-805D-58DE936B984C").unwrap();
    let plugin = store.product(plugin_id).unwrap().unwrap();
    assert_eq!(plugin.current_version.as_deref(), Some("1.0.24"));
    assert_eq!(products[0].svn_username.as_deref(), Some("acme"));
    assert_eq!(products[0].validation.package_size_warning_mb, 25);

    let releases = store.releases(None).unwrap();
    assert_eq!(releases.len(), 3);
    let created: Vec<f64> = releases.iter().map(|r| r.created_at.0).collect();
    assert!(created.windows(2).all(|w| w[0] >= w[1]), "newest first");
    assert_eq!(store.releases(Some(plugin_id)).unwrap().len(), 2);
    assert!(store.active_releases().unwrap().is_empty(), "all fixtures are terminal");
    for release in &releases {
        let events = store.events(release.id).unwrap();
        assert_eq!(events.len(), release.transitions.len());
        for (event, transition) in events.iter().zip(&release.transitions) {
            assert_eq!(event.to, transition.to);
            assert_eq!(event.from, transition.from);
            assert_eq!(event.note, transition.note);
            assert!(
                (event.at.0 - transition.at.0).abs() < 1e-5,
                "event time from seconds since 1970"
            );
        }
    }
    assert_eq!(
        store.setting::<String>("update.notifiedVersion").unwrap().as_deref(),
        Some("0.2.3")
    );
    let settings: Value = store.setting("app.settings").unwrap().unwrap();
    assert_eq!(settings["url"], "https://example.org/feed");

    // Writing back unchanged records reproduces Swift's bytes and key columns.
    for product in &products {
        store.save_product(product).unwrap();
    }
    for release in &releases {
        store.save_release(release).unwrap();
    }
    let (data, created_at, updated_at) = raw_row(
        &path,
        "SELECT data, created_at, updated_at FROM products WHERE slug = 'sample-plugin'",
    );
    assert_eq!(String::from_utf8(data).unwrap(), SWIFT_PRODUCT);
    assert!((created_at - (817029777.884127 + REFERENCE_OFFSET)).abs() < 1e-5);
    assert!((updated_at - (817293629.069138 + REFERENCE_OFFSET)).abs() < 1e-5);
    let theme = raw_row(
        &path,
        "SELECT data, created_at, updated_at FROM products WHERE slug = 'classified-listing-toolkits'",
    );
    assert_eq!(String::from_utf8(theme.0).unwrap(), SWIFT_TOOLKITS);
    for fixture in FIXTURES {
        let id = serde_json::from_str::<Value>(fixture).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let (data, ..) = raw_row(
            &path,
            &format!("SELECT data, created_at, updated_at FROM releases WHERE id = '{id}'"),
        );
        assert_eq!(String::from_utf8(data).unwrap(), swift_compact(fixture), "{id}");
    }
    let event_count = |store: &Store| store.db.query("SELECT COUNT(*) FROM release_events", &[]).unwrap()[0][0].clone();
    assert_eq!(event_count(&store), SqlValue::Integer(9 + 3 + 6), "no duplicate events");

    // Write changes: a new product, an updated product, a new release with transitions.
    let mut updated = plugin.clone();
    updated.current_version = Some("1.0.25".into());
    updated.updated_at = SwiftDate(817400000.25);
    store.save_product(&updated).unwrap();
    let mut new_product = Product::new(
        "Another",
        "another-plugin",
        ProductType::Plugin,
        "https://plugins.svn.wordpress.org/another-plugin",
        "/tmp/svn/another",
    );
    new_product.svn_username = Some("alice".into());
    store.save_product(&new_product).unwrap();
    let mut release = ReleaseRecord::new(
        &updated,
        rd_records::ReleaseSource::folder("/dev/sample"),
        rd_model::ReleaseMode::Assisted,
    );
    release.version = Some("1.0.25".into());
    store.save_release(&release).unwrap();
    ReleaseStateMachine::transition(&mut release, ReleaseState::Inspected, Some("checked")).unwrap();
    store.save_release(&release).unwrap();
    store.set_setting("0.2.4", "update.notifiedVersion").unwrap();
    drop(store);

    // Re-read with a fresh store.
    let store = Store::open(&paths).unwrap();
    assert_eq!(store.all_products().unwrap().len(), 3);
    assert_eq!(
        store.product(plugin_id).unwrap().unwrap().current_version.as_deref(),
        Some("1.0.25")
    );
    assert_eq!(store.product(new_product.id).unwrap().unwrap(), new_product);
    let reread = store.release(release.id).unwrap().unwrap();
    assert_eq!(reread, release);
    assert_eq!(store.active_releases().unwrap(), [release.clone()]);
    assert_eq!(store.events(release.id).unwrap().len(), 2);
    assert_eq!(
        store.setting::<String>("update.notifiedVersion").unwrap().as_deref(),
        Some("0.2.4")
    );

    // What Swift will read: key columns in seconds since 1970, uppercase ids, Swift JSON.
    let (data, created_at, updated_at) = raw_row(
        &path,
        "SELECT data, created_at, updated_at FROM products WHERE slug = 'sample-plugin'",
    );
    let json: Value = serde_json::from_slice(&data).unwrap();
    assert_eq!(json["currentVersion"], "1.0.25");
    assert!((created_at - (817029777.884127 + REFERENCE_OFFSET)).abs() < 1e-5);
    assert!((updated_at - (817400000.25 + REFERENCE_OFFSET)).abs() < 1e-5);
    let db = Connection::open(&path).unwrap();
    let (id, product_id, version, state, at): (String, String, String, String, f64) = db
        .query_row(
            "SELECT r.id, r.product_id, r.version, r.state, e.at FROM releases r JOIN release_events e ON e.release_id = r.id WHERE r.id = ? ORDER BY e.seq DESC LIMIT 1",
            [release.id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
        )
        .unwrap();
    assert_eq!(id, id.to_uppercase());
    assert_eq!(product_id, "EE90AEAA-DAB2-45F4-805D-58DE936B984C");
    assert_eq!((version.as_str(), state.as_str()), ("1.0.25", "inspected"));
    assert!((at - release.transitions[1].at.unix_seconds()).abs() < 1e-5);
    let text = String::from_utf8(data).unwrap();
    assert!(text.contains(r#""svnRepositoryURL":"https:\/\/plugins.svn.wordpress.org\/sample-plugin""#));
}
