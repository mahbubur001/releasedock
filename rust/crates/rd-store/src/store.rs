//! Port of `Persistence/Store.swift`.

use crate::app_paths::AppPaths;
use crate::database::{DatabaseError, SqlValue, SqliteDatabase};
use crate::migrations::Migrator;
use rd_model::{Id, Product, ProductType, SwiftDate};
use rd_records::{ReleaseRecord, ReleaseState, StateTransition};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::path::Path;

pub trait ProductRepository: Send + Sync {
    fn all_products(&self) -> Result<Vec<Product>, StoreError>;
    fn product(&self, id: Id) -> Result<Option<Product>, StoreError>;
    /// Swift's `save(_ product:)`.
    fn save_product(&self, product: &Product) -> Result<(), StoreError>;
    fn delete_product(&self, id: Id) -> Result<(), StoreError>;
}

pub trait ReleaseRepository: Send + Sync {
    /// Inserts or updates the release and appends any new state transitions to the event log.
    /// Swift's `save(_ release:)`.
    fn save_release(&self, release: &ReleaseRecord) -> Result<(), StoreError>;
    fn release(&self, id: Id) -> Result<Option<ReleaseRecord>, StoreError>;
    fn releases(&self, product_id: Option<Id>) -> Result<Vec<ReleaseRecord>, StoreError>;
    /// Releases that are not in a terminal state.
    fn active_releases(&self) -> Result<Vec<ReleaseRecord>, StoreError>;
}

/// Settings are JSON values stored under a key. The raw methods keep the trait usable as
/// `dyn SettingsRepository`; the typed ones are Swift's generic `setting(_:forKey:)` and
/// `setSetting(_:forKey:)`.
pub trait SettingsRepository: Send + Sync {
    /// The stored JSON for `key`, if any.
    fn setting_data(&self, key: &str) -> Result<Option<Vec<u8>>, StoreError>;
    /// Stores JSON produced by [`encode`] under `key`.
    fn set_setting_data(&self, key: &str, data: &[u8]) -> Result<(), StoreError>;

    fn setting<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>, StoreError>
    where
        Self: Sized,
    {
        match self.setting_data(key)? {
            Some(data) => serde_json::from_slice(&data)
                .map(Some)
                .map_err(|e| StoreError::Decoding(e.to_string())),
            None => Ok(None),
        }
    }

    fn set_setting<T: Serialize + ?Sized>(&self, value: &T, key: &str) -> Result<(), StoreError>
    where
        Self: Sized,
    {
        self.set_setting_data(key, &encode(value)?)
    }
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum StoreError {
    #[error("A {} with the slug \"{slug}\" is already registered.", product_type_raw(*product_type))]
    DuplicateProduct { product_type: ProductType, slug: String },
    #[error("Stored record {0} could not be decoded.")]
    CorruptRecord(String),
    #[error(transparent)]
    Database(#[from] DatabaseError),
    /// A value could not be encoded as JSON (Swift's `EncodingError`).
    #[error("{0}")]
    Encoding(String),
    /// A setting could not be decoded (Swift's `DecodingError`).
    #[error("{0}")]
    Decoding(String),
    /// Creating the data folders failed (Swift rethrows the `FileManager` error).
    #[error("{0}")]
    Io(String),
}

pub(crate) fn product_type_raw(product_type: ProductType) -> &'static str {
    match product_type {
        ProductType::Plugin => "plugin",
        ProductType::Theme => "theme",
    }
}

/// The raw value of a Swift string enum (its serde representation).
pub(crate) fn raw_value<T: Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(s)) => s,
        _ => String::new(),
    }
}

fn release_state(raw: &str) -> Option<ReleaseState> {
    serde_json::from_value(serde_json::Value::String(raw.to_owned())).ok()
}

/// Encodes a value like the Swift store's `JSONEncoder` with `.sortedKeys`.
pub fn encode<T: Serialize + ?Sized>(value: &T) -> Result<Vec<u8>, StoreError> {
    let value = serde_json::to_value(value).map_err(|e| StoreError::Encoding(e.to_string()))?;
    Ok(rd_model::swift_json::to_compact(&value).into_bytes())
}

/// SQLite-backed persistence for products, releases (with an append-only state
/// event log) and settings. Records are stored as JSON with indexed key columns.
///
/// The `created_at`, `updated_at` and `at` columns hold seconds since 1970 (Swift's
/// `timeIntervalSince1970`); dates inside the JSON are seconds since 2001 ([`SwiftDate`]).
pub struct Store {
    pub db: SqliteDatabase,
}

impl Store {
    pub fn new(database: SqliteDatabase, backup_directory: Option<&Path>) -> Result<Self, StoreError> {
        Migrator::migrate(&database, backup_directory, SwiftDate::now())?;
        Ok(Self { db: database })
    }

    /// Opens the on-disk store in the standard data directory.
    pub fn open(paths: &AppPaths) -> Result<Self, StoreError> {
        paths.create_all().map_err(|e| StoreError::Io(e.to_string()))?;
        let backups = paths.backups();
        Self::new(SqliteDatabase::new(Some(&paths.database_file()))?, Some(&backups))
    }

    pub fn in_memory() -> Result<Self, StoreError> {
        Self::new(SqliteDatabase::new(None)?, None)
    }

    /// Raw event log for a release, for diagnostics.
    pub fn events(&self, release_id: Id) -> Result<Vec<StateTransition>, StoreError> {
        let id = release_id.to_string();
        self.db
            .query(
                "SELECT from_state, to_state, note, at FROM release_events WHERE release_id = ? ORDER BY seq",
                &[SqlValue::Text(id.clone())],
            )?
            .into_iter()
            .map(|row| {
                let to = row[1].string_value().as_deref().and_then(release_state);
                match (to, &row[3]) {
                    (Some(to), SqlValue::Real(at)) => Ok(StateTransition::new(
                        row[0].string_value().as_deref().and_then(release_state),
                        to,
                        SwiftDate::from_unix_seconds(*at),
                        row[2].string_value(),
                    )),
                    _ => Err(StoreError::CorruptRecord(id.clone())),
                }
            })
            .collect()
    }

    fn decode<T: DeserializeOwned>(row: &[SqlValue]) -> Result<T, StoreError> {
        let first = || {
            row.first()
                .and_then(SqlValue::string_value)
                .unwrap_or_else(|| "?".into())
        };
        let data = match row {
            [_, data] => data.data_value().ok_or_else(|| StoreError::CorruptRecord(first()))?,
            _ => return Err(StoreError::CorruptRecord(first())),
        };
        serde_json::from_slice(data).map_err(|error| {
            log::error!("Failed to decode {}: {error}", std::any::type_name::<T>());
            StoreError::CorruptRecord(first())
        })
    }

    fn decode_all<T: DeserializeOwned>(rows: Vec<Vec<SqlValue>>) -> Result<Vec<T>, StoreError> {
        rows.iter().map(|row| Self::decode(row)).collect()
    }
}

// MARK: Products

impl ProductRepository for Store {
    fn all_products(&self) -> Result<Vec<Product>, StoreError> {
        Self::decode_all(self.db.query("SELECT id, data FROM products ORDER BY slug", &[])?)
    }

    fn product(&self, id: Id) -> Result<Option<Product>, StoreError> {
        let rows = self.db.query(
            "SELECT id, data FROM products WHERE id = ?",
            &[SqlValue::Text(id.to_string())],
        )?;
        rows.first().map(|row| Self::decode(row)).transpose()
    }

    fn save_product(&self, product: &Product) -> Result<(), StoreError> {
        let product_type = product_type_raw(product.product_type);
        let conflict = self.db.query(
            "SELECT id FROM products WHERE type = ? AND slug = ? AND id <> ?",
            &[
                SqlValue::Text(product_type.into()),
                SqlValue::Text(product.slug.clone()),
                SqlValue::Text(product.id.to_string()),
            ],
        )?;
        if !conflict.is_empty() {
            return Err(StoreError::DuplicateProduct {
                product_type: product.product_type,
                slug: product.slug.clone(),
            });
        }
        self.db.run(
            "INSERT INTO products (id, type, slug, data, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET type = excluded.type, slug = excluded.slug, data = excluded.data, updated_at = excluded.updated_at",
            &[
                SqlValue::Text(product.id.to_string()),
                SqlValue::Text(product_type.into()),
                SqlValue::Text(product.slug.clone()),
                SqlValue::Blob(encode(product)?),
                SqlValue::Real(product.created_at.unix_seconds()),
                SqlValue::Real(product.updated_at.unix_seconds()),
            ],
        )?;
        Ok(())
    }

    fn delete_product(&self, id: Id) -> Result<(), StoreError> {
        self.db
            .run("DELETE FROM products WHERE id = ?", &[SqlValue::Text(id.to_string())])?;
        Ok(())
    }
}

// MARK: Releases

impl ReleaseRepository for Store {
    fn save_release(&self, release: &ReleaseRecord) -> Result<(), StoreError> {
        let data = encode(release)?;
        let id = release.id.to_string();
        self.db.transaction(|tx| {
            tx.run(
                "INSERT INTO releases (id, product_id, version, state, data, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT(id) DO UPDATE SET version = excluded.version, state = excluded.state, data = excluded.data, updated_at = excluded.updated_at",
                &[
                    SqlValue::Text(id.clone()),
                    SqlValue::Text(release.product_id.to_string()),
                    SqlValue::optional_text(release.version.as_deref()),
                    SqlValue::Text(raw_value(&release.state)),
                    SqlValue::Blob(data),
                    SqlValue::Real(release.created_at.unix_seconds()),
                    SqlValue::Real(release.updated_at.unix_seconds()),
                ],
            )?;
            let stored = tx
                .query(
                    "SELECT COUNT(*) FROM release_events WHERE release_id = ?",
                    &[SqlValue::Text(id.clone())],
                )?
                .first()
                .and_then(|row| row.first())
                .and_then(SqlValue::int_value)
                .unwrap_or(0);
            for transition in release.transitions.iter().skip(usize::try_from(stored).unwrap_or(0)) {
                tx.run(
                    "INSERT INTO release_events (release_id, from_state, to_state, note, at) VALUES (?, ?, ?, ?, ?)",
                    &[
                        SqlValue::Text(id.clone()),
                        transition.from.map_or(SqlValue::Null, |s| SqlValue::Text(raw_value(&s))),
                        SqlValue::Text(raw_value(&transition.to)),
                        SqlValue::optional_text(transition.note.as_deref()),
                        SqlValue::Real(transition.at.unix_seconds()),
                    ],
                )?;
            }
            Ok::<(), StoreError>(())
        })
    }

    fn release(&self, id: Id) -> Result<Option<ReleaseRecord>, StoreError> {
        let rows = self.db.query(
            "SELECT id, data FROM releases WHERE id = ?",
            &[SqlValue::Text(id.to_string())],
        )?;
        rows.first().map(|row| Self::decode(row)).transpose()
    }

    fn releases(&self, product_id: Option<Id>) -> Result<Vec<ReleaseRecord>, StoreError> {
        let rows = match product_id {
            Some(product_id) => self.db.query(
                "SELECT id, data FROM releases WHERE product_id = ? ORDER BY created_at DESC",
                &[SqlValue::Text(product_id.to_string())],
            )?,
            None => self
                .db
                .query("SELECT id, data FROM releases ORDER BY created_at DESC", &[])?,
        };
        Self::decode_all(rows)
    }

    fn active_releases(&self) -> Result<Vec<ReleaseRecord>, StoreError> {
        let terminal = [ReleaseState::Completed, ReleaseState::Failed, ReleaseState::Cancelled]
            .iter()
            .map(|s| format!("'{}'", raw_value(s)))
            .collect::<Vec<_>>()
            .join(",");
        Self::decode_all(self.db.query(
            &format!("SELECT id, data FROM releases WHERE state NOT IN ({terminal}) ORDER BY created_at DESC"),
            &[],
        )?)
    }
}

// MARK: Settings

impl SettingsRepository for Store {
    fn setting_data(&self, key: &str) -> Result<Option<Vec<u8>>, StoreError> {
        let rows = self.db.query(
            "SELECT value FROM settings WHERE key = ?",
            &[SqlValue::Text(key.into())],
        )?;
        Ok(rows
            .first()
            .and_then(|row| row.first())
            .and_then(SqlValue::data_value)
            .map(<[u8]>::to_vec))
    }

    fn set_setting_data(&self, key: &str, data: &[u8]) -> Result<(), StoreError> {
        self.db.run(
            "INSERT INTO settings (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            &[SqlValue::Text(key.into()), SqlValue::Blob(data.to_vec())],
        )?;
        Ok(())
    }
}
