//! Persistence and product management. Ported from `ReleaseDockCore`:
//! `Persistence/SQLiteDatabase.swift`, `Persistence/Migrations.swift`, `Persistence/Store.swift`,
//! `Support/AppPaths.swift`, `Services/ProductService.swift` and `Services/DiagnosticsService.swift`.
//!
//! The store opens the Swift app's database unchanged: same tables, indexes, PRAGMAs and
//! `user_version`, and the same JSON in the `data` columns (sorted keys, escaped slashes;
//! see `rd_model::swift_json`). Key columns hold seconds since 1970; dates inside the JSON are
//! seconds since 2001.

pub mod app_paths;
pub mod database;
pub mod diagnostics;
pub mod migrations;
pub mod product_service;
pub mod store;

pub use app_paths::{AppPaths, AppPathsError, DATA_DIR_VARIABLE};
pub use database::{DatabaseError, Executor, SqlValue, SqliteDatabase};
pub use diagnostics::{DiagnosticsError, DiagnosticsService, LogEntry, LogSource};
pub use migrations::{INITIAL_SCHEMA, Migration, Migrator};
pub use product_service::{
    ProductField, ProductService, ProductServiceError, ProductServicing, ProductSuggestion, ProductValidationIssue,
};
pub use store::{ProductRepository, ReleaseRepository, SettingsRepository, Store, StoreError, encode};
