//! Manual check against a COPY of a database written by the Swift app:
//!
//! ```sh
//! sqlite3 "$HOME/Library/Application Support/ReleaseDock/ReleaseDockData/Database/releasedock.sqlite" \
//!     ".backup /tmp/rd-store-check.sqlite"
//! RD_STORE_REAL_DB=/tmp/rd-store-check.sqlite cargo test -p rd-store --test real_database -- --ignored --nocapture
//! ```
//!
//! The given file is only opened read-only (`immutable=1`); the store works on a second copy in
//! a temporary folder. Nothing from the database is printed except counts and key paths.
#![allow(clippy::unwrap_used)]

mod common;

use common::TempDir;
use rd_model::Id;
use rd_store::*;
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;

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

/// Key paths where two JSON trees differ (numbers compare by value).
fn differences(expected: &Value, actual: &Value, path: &str, out: &mut Vec<String>) {
    match (expected, actual) {
        (Value::Object(e), Value::Object(a)) => {
            for key in e.keys().chain(a.keys().filter(|k| !e.contains_key(*k))) {
                match (e.get(key), a.get(key)) {
                    (Some(e), Some(a)) => differences(e, a, &format!("{path}.{key}"), out),
                    _ => out.push(format!("{path}.{key}")),
                }
            }
        }
        (Value::Array(e), Value::Array(a)) if e.len() == a.len() => {
            for (index, (e, a)) in e.iter().zip(a).enumerate() {
                differences(e, a, &format!("{path}[{index}]"), out);
            }
        }
        (Value::Number(e), Value::Number(a)) if e.as_f64() == a.as_f64() => {}
        _ if expected == actual => {}
        _ => out.push(path.to_owned()),
    }
}

struct Tally {
    rows: usize,
    identical_bytes: usize,
    differing_paths: Vec<String>,
}

fn compare(original: &[u8], encoded: &[u8], tally: &mut Tally) {
    tally.rows += 1;
    if original == encoded {
        tally.identical_bytes += 1;
        return;
    }
    let (e, a): (Value, Value) = (
        serde_json::from_slice(original).unwrap(),
        serde_json::from_slice(encoded).unwrap(),
    );
    let mut paths = Vec::new();
    differences(&e, &a, "", &mut paths);
    tally.differing_paths.extend(paths);
}

#[test]
#[ignore = "needs RD_STORE_REAL_DB pointing at a copy of a Swift database"]
fn opens_a_copy_of_the_real_swift_database() {
    let Ok(source) = std::env::var("RD_STORE_REAL_DB") else {
        eprintln!("RD_STORE_REAL_DB is not set; skipping");
        return;
    };
    let before = std::fs::read(&source).unwrap();

    // 1. Schema and version, read-only.
    let real = Connection::open_with_flags(
        format!("file:{source}?immutable=1"),
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )
    .unwrap();
    let dir = TempDir::new();
    drop(Store::open(&AppPaths::new(dir.path("fresh"))).unwrap());
    let fresh = Connection::open(dir.path("fresh/Database/releasedock.sqlite")).unwrap();
    assert_eq!(schema(&real), schema(&fresh), "same tables, columns and indexes");
    let version = |db: &Connection| db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0)).unwrap();
    assert_eq!(version(&real), Migrator::latest_version());
    drop(real);

    // 2. The store on a second copy.
    let paths = AppPaths::new(dir.path("copy"));
    paths.create_all().unwrap();
    std::fs::write(paths.database_file(), &before).unwrap();
    let store = Store::open(&paths).unwrap();
    assert!(
        std::fs::read_dir(paths.backups()).unwrap().next().is_none(),
        "no migration ran"
    );

    let mut products = Tally {
        rows: 0,
        identical_bytes: 0,
        differing_paths: Vec::new(),
    };
    for row in store.db.query("SELECT id, data FROM products", &[]).unwrap() {
        let id = Id::parse(&row[0].string_value().unwrap()).unwrap();
        let product = store.product(id).unwrap().unwrap();
        compare(row[1].data_value().unwrap(), &encode(&product).unwrap(), &mut products);
    }
    let mut releases = Tally {
        rows: 0,
        identical_bytes: 0,
        differing_paths: Vec::new(),
    };
    let mut events_match = 0;
    for row in store.db.query("SELECT id, data FROM releases", &[]).unwrap() {
        let id = Id::parse(&row[0].string_value().unwrap()).unwrap();
        let release = store.release(id).unwrap().unwrap();
        compare(row[1].data_value().unwrap(), &encode(&release).unwrap(), &mut releases);
        let events = store.events(id).unwrap();
        if events.iter().map(|e| e.to).eq(release.transitions.iter().map(|t| t.to)) {
            events_match += 1;
        }
    }
    let mut settings = Tally {
        rows: 0,
        identical_bytes: 0,
        differing_paths: Vec::new(),
    };
    for row in store.db.query("SELECT key, value FROM settings", &[]).unwrap() {
        let value: Value = store.setting(&row[0].string_value().unwrap()).unwrap().unwrap();
        compare(row[1].data_value().unwrap(), &encode(&value).unwrap(), &mut settings);
    }
    assert_eq!(store.all_products().unwrap().len(), products.rows);
    assert_eq!(store.releases(None).unwrap().len(), releases.rows);
    let active = store.active_releases().unwrap().len();

    // 3. Write everything back and read it again.
    let event_count = |store: &Store| store.db.query("SELECT COUNT(*) FROM release_events", &[]).unwrap()[0][0].clone();
    let events_before = event_count(&store);
    let all_products = store.all_products().unwrap();
    let all_releases = store.releases(None).unwrap();
    for product in &all_products {
        store.save_product(product).unwrap();
    }
    for release in &all_releases {
        store.save_release(release).unwrap();
    }
    drop(store);
    let store = Store::open(&paths).unwrap();
    assert_eq!(store.all_products().unwrap(), all_products);
    assert_eq!(store.releases(None).unwrap(), all_releases);
    assert_eq!(event_count(&store), events_before, "rewriting adds no events");

    assert_eq!(std::fs::read(&source).unwrap(), before, "the source copy is untouched");

    let differing = |t: &Tally| {
        let mut paths = t.differing_paths.clone();
        paths.sort();
        paths.dedup();
        paths
    };
    eprintln!(
        "products: {} rows, {} byte-identical when re-encoded, differing keys {:?}",
        products.rows,
        products.identical_bytes,
        differing(&products)
    );
    eprintln!(
        "releases: {} rows ({active} active), {} byte-identical, differing keys {:?}; events match transitions for {events_match}",
        releases.rows,
        releases.identical_bytes,
        differing(&releases)
    );
    eprintln!(
        "settings: {} rows, {} byte-identical, differing keys {:?}",
        settings.rows,
        settings.identical_bytes,
        differing(&settings)
    );
    eprintln!("events: {events_before:?}; schema and user_version match; write + reread ok");
}
