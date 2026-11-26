//! `StoreTests` from CoreTests.swift, plus migration upgrade/rollback and settings encoding.
#![allow(clippy::unwrap_used)]

mod common;

use common::{TempDir, default_product};
use rd_model::{Id, SwiftDate};
use rd_records::{ReleaseRecord, ReleaseSource, ReleaseState, ReleaseStateMachine};
use rd_store::*;
use std::collections::BTreeMap;

#[test]
fn products_releases_and_events() {
    let store = Store::in_memory().unwrap();
    let mut product = default_product();
    store.save_product(&product).unwrap();
    product.current_version = Some("1.0.0".into());
    store.save_product(&product).unwrap();
    let versions: Vec<_> = store
        .all_products()
        .unwrap()
        .into_iter()
        .map(|p| p.current_version)
        .collect();
    assert_eq!(versions, [Some("1.0.0".to_owned())]);

    let mut duplicate = default_product();
    duplicate.id = Id::new();
    let error = store.save_product(&duplicate).unwrap_err();
    assert!(matches!(error, StoreError::DuplicateProduct { .. }));
    assert_eq!(
        error.to_string(),
        "A plugin with the slug \"sample-plugin\" is already registered."
    );

    let mut record = ReleaseRecord::new(&product, ReleaseSource::zip("/p.zip"), rd_model::ReleaseMode::Assisted);
    store.save_release(&record).unwrap();
    ReleaseStateMachine::transition(&mut record, ReleaseState::Inspected, Some("ok")).unwrap();
    store.save_release(&record).unwrap();
    ReleaseStateMachine::transition(&mut record, ReleaseState::Validated, None).unwrap();
    store.save_release(&record).unwrap();
    store.save_release(&record).unwrap(); // idempotent: no duplicate events
    let events: Vec<_> = store.events(record.id).unwrap().into_iter().map(|e| e.to).collect();
    assert_eq!(
        events,
        [ReleaseState::Detected, ReleaseState::Inspected, ReleaseState::Validated]
    );
    let active: Vec<_> = store.active_releases().unwrap().into_iter().map(|r| r.id).collect();
    assert_eq!(active, [record.id]);
    assert_eq!(
        store.release(record.id).unwrap().map(|r| r.state),
        Some(ReleaseState::Validated)
    );

    store.set_setting(&BTreeMap::from([("a", 1)]), "k").unwrap();
    assert_eq!(
        store.setting::<BTreeMap<String, i64>>("k").unwrap(),
        Some(BTreeMap::from([("a".to_owned(), 1)]))
    );

    store.delete_product(product.id).unwrap();
    assert!(
        store.release(record.id).unwrap().is_none(),
        "releases cascade with their product"
    );
}

#[test]
fn migrations_backup_and_refuse_newer_schema() {
    let dir = TempDir::new();
    let paths = AppPaths::new(dir.path("data"));
    drop(Store::open(&paths).unwrap());
    let db = SqliteDatabase::new(Some(&paths.database_file())).unwrap();
    assert_eq!(db.user_version().unwrap(), Migrator::latest_version());
    db.execute("PRAGMA user_version = 999").unwrap();
    let backups = paths.backups();
    let error = Store::new(db, Some(&backups)).err().unwrap();
    assert!(
        matches!(error, StoreError::Database(DatabaseError::NewerSchema { .. })),
        "{error}"
    );
    assert_eq!(
        error.to_string(),
        "The database was created by a newer ReleaseDock (schema 999, this version supports 1). Update ReleaseDock."
    );
}

#[cfg(unix)]
#[test]
fn database_file_is_private() {
    use std::os::unix::fs::PermissionsExt;
    let dir = TempDir::new();
    let paths = AppPaths::new(dir.path("data"));
    drop(Store::open(&paths).unwrap());
    let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&paths.database_file()), 0o600);
    assert_eq!(mode(&paths.root), 0o700);
}

// Not in Swift: the upgrade path of `Migrator.migrate` (Swift has a single migration, so its
// backup and rollback branches are never exercised by the Swift tests).

const ADD_NOTES: Migration = Migration {
    version: 2,
    name: "notes",
    apply: |db| db.execute("CREATE TABLE notes (id INTEGER PRIMARY KEY)"),
};

const BROKEN: Migration = Migration {
    version: 2,
    name: "broken",
    apply: |db| {
        db.execute("CREATE TABLE notes (id INTEGER PRIMARY KEY)")?;
        db.execute("THIS IS NOT SQL")
    },
};

#[test]
fn upgrading_an_existing_database_writes_a_backup_first() {
    let dir = TempDir::new();
    let paths = AppPaths::new(dir.path("data"));
    let store = Store::open(&paths).unwrap();
    store.save_product(&default_product()).unwrap();
    drop(store);

    let db = SqliteDatabase::new(Some(&paths.database_file())).unwrap();
    // 2026-11-26T09:30:00Z
    let now = SwiftDate::from_unix_seconds(1_795_685_400.0);
    let migrations = [Migrator::MIGRATIONS[0], ADD_NOTES];
    Migrator::migrate_with(&migrations, &db, Some(&paths.backups()), now).unwrap();
    assert_eq!(db.user_version().unwrap(), 2);
    let backup = paths.backups().join("releasedock-v1-2026-11-26T09-30-00Z.sqlite");
    assert!(backup.exists(), "backup named after the old version and the time");

    let old = SqliteDatabase::new(Some(&backup)).unwrap();
    assert_eq!(old.user_version().unwrap(), 1);
    let count = old.query("SELECT COUNT(*) FROM products", &[]).unwrap();
    assert_eq!(count[0][0], SqlValue::Integer(1), "the backup holds the data");
}

#[test]
fn a_failed_migration_rolls_back_and_names_the_version() {
    let db = SqliteDatabase::new(None).unwrap();
    Migrator::migrate(&db, None, SwiftDate::now()).unwrap();
    let migrations = [Migrator::MIGRATIONS[0], BROKEN];
    let error = Migrator::migrate_with(&migrations, &db, None, SwiftDate::now()).unwrap_err();
    assert!(matches!(error, DatabaseError::Migration { version: 2, .. }), "{error}");
    assert!(
        error.to_string().starts_with("Database migration 2 failed: "),
        "{error}"
    );
    assert_eq!(db.user_version().unwrap(), 1);
    let tables = db
        .query("SELECT name FROM sqlite_master WHERE name = 'notes'", &[])
        .unwrap();
    assert!(tables.is_empty(), "the partial migration was rolled back");
}

#[test]
fn file_databases_use_wal_foreign_keys_and_full_sync() {
    let dir = TempDir::new();
    let db = SqliteDatabase::new(Some(&dir.path("x.sqlite"))).unwrap();
    let pragma = |name: &str| db.query(&format!("PRAGMA {name}"), &[]).unwrap()[0][0].clone();
    assert_eq!(pragma("journal_mode"), SqlValue::Text("wal".into()));
    assert_eq!(pragma("foreign_keys"), SqlValue::Integer(1));
    assert_eq!(pragma("synchronous"), SqlValue::Integer(2));
}

#[test]
fn settings_are_stored_as_swift_json() {
    let store = Store::in_memory().unwrap();
    store.set_setting("0.2.3", "update.notifiedVersion").unwrap();
    store
        .set_setting(
            &serde_json::json!({"url": "https://a/b", "b": 2.0, "a": 0.5}),
            "complex",
        )
        .unwrap();
    assert_eq!(
        store.setting_data("update.notifiedVersion").unwrap(),
        Some(b"\"0.2.3\"".to_vec())
    );
    assert_eq!(
        String::from_utf8(store.setting_data("complex").unwrap().unwrap()).unwrap(),
        r#"{"a":0.5,"b":2,"url":"https:\/\/a\/b"}"#
    );
    assert_eq!(
        store.setting::<String>("update.notifiedVersion").unwrap().as_deref(),
        Some("0.2.3")
    );
    assert_eq!(store.setting::<String>("missing").unwrap(), None);
    assert!(matches!(
        store.setting::<i64>("update.notifiedVersion"),
        Err(StoreError::Decoding(_))
    ));
}

#[test]
fn undecodable_rows_are_reported_as_corrupt_records() {
    let store = Store::in_memory().unwrap();
    store
        .db
        .run(
            "INSERT INTO products (id, type, slug, data, created_at, updated_at) VALUES ('X', 'plugin', 's', '{}', 0, 0)",
            &[],
        )
        .unwrap();
    let error = store.all_products().unwrap_err();
    assert_eq!(error, StoreError::CorruptRecord("X".into()));
    assert_eq!(error.to_string(), "Stored record X could not be decoded.");
}
