//! Port of `Persistence/Migrations.swift`.

use crate::database::{DatabaseError, Executor, SqliteDatabase};
use rd_model::{SwiftDate, iso8601};
use std::fs;
use std::path::Path;

/// Ordered schema migrations tracked with `PRAGMA user_version`. Each migration runs
/// in its own transaction; before migrating an existing database a copy is written
/// to `Backups/` so a failed upgrade can be rolled back by hand.
#[derive(Clone, Copy)]
pub struct Migration {
    pub version: i64,
    pub name: &'static str,
    pub apply: fn(&Executor<'_>) -> Result<(), DatabaseError>,
}

/// The schema of version 1, exactly as the Swift app creates it (the text is stored in
/// `sqlite_master`, so the indentation matches Swift's multi-line literal too).
pub const INITIAL_SCHEMA: &str = "\
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
);";

pub struct Migrator;

impl Migrator {
    pub const MIGRATIONS: &'static [Migration] = &[Migration {
        version: 1,
        name: "initial schema",
        apply: |db| db.execute(INITIAL_SCHEMA),
    }];

    pub fn latest_version() -> i64 {
        latest_version(Self::MIGRATIONS)
    }

    /// Brings the database to the latest schema. Refuses to open a database written by
    /// a newer app version rather than risk corrupting it.
    pub fn migrate(db: &SqliteDatabase, backup_directory: Option<&Path>, now: SwiftDate) -> Result<(), DatabaseError> {
        Self::migrate_with(Self::MIGRATIONS, db, backup_directory, now)
    }

    /// [`Migrator::migrate`] with an explicit list of migrations (tests use it to exercise
    /// the backup and rollback of an upgrade).
    pub fn migrate_with(
        migrations: &[Migration],
        db: &SqliteDatabase,
        backup_directory: Option<&Path>,
        now: SwiftDate,
    ) -> Result<(), DatabaseError> {
        let current = db.user_version()?;
        let latest = latest_version(migrations);
        if current > latest {
            return Err(DatabaseError::NewerSchema {
                found: current,
                supported: latest,
            });
        }
        let mut pending: Vec<&Migration> = migrations.iter().filter(|m| m.version > current).collect();
        pending.sort_by_key(|m| m.version);
        if pending.is_empty() {
            return Ok(());
        }

        if let (true, Some(source), Some(backup_directory)) = (current > 0, db.path(), backup_directory) {
            db.execute("PRAGMA wal_checkpoint(TRUNCATE)")?;
            let stamp = iso8601::format(now).replace(':', "-");
            let backup = backup_directory.join(format!("releasedock-v{current}-{stamp}.sqlite"));
            fs::create_dir_all(backup_directory).map_err(|e| DatabaseError::Io(e.to_string()))?;
            // `FileManager.copyItem` refuses to overwrite an existing file.
            if backup.exists() {
                return Err(DatabaseError::Io(format!("{} already exists", backup.display())));
            }
            fs::copy(source, &backup).map_err(|e| DatabaseError::Io(e.to_string()))?;
            log::info!(
                "Backed up database before migration to {}",
                backup.file_name().map(|n| n.to_string_lossy()).unwrap_or_default()
            );
        }

        for migration in pending {
            db.transaction(|tx| {
                (migration.apply)(tx)?;
                tx.execute(&format!("PRAGMA user_version = {}", migration.version))
            })
            .map_err(|error: DatabaseError| DatabaseError::Migration {
                version: migration.version,
                message: error.to_string(),
            })?;
            log::info!("Applied migration {} ({})", migration.version, migration.name);
        }
        Ok(())
    }
}

fn latest_version(migrations: &[Migration]) -> i64 {
    migrations.iter().map(|m| m.version).max().unwrap_or(0)
}
