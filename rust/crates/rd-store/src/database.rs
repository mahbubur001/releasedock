//! Port of `Persistence/SQLiteDatabase.swift`.

use rusqlite::types::{ToSqlOutput, ValueRef};
use rusqlite::{Connection, OpenFlags, ToSql, TransactionBehavior};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DatabaseError {
    #[error("Could not open the database: {0}")]
    Open(String),
    /// The message and the first 80 characters of the statement.
    #[error("SQL error: {message} in {}", sql.chars().take(80).collect::<String>())]
    Prepare { message: String, sql: String },
    #[error("SQL execution error: {0}")]
    Step(String),
    #[error("Database migration {version} failed: {message}")]
    Migration { version: i64, message: String },
    #[error(
        "The database was created by a newer ReleaseDock (schema {found}, this version supports {supported}). Update ReleaseDock."
    )]
    NewerSchema { found: i64, supported: i64 },
    /// Copying the database before a migration failed (Swift rethrows the `FileManager` error).
    #[error("{0}")]
    Io(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum SqlValue {
    Null,
    Integer(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}

impl SqlValue {
    pub fn string_value(&self) -> Option<String> {
        match self {
            Self::Text(s) => Some(s.clone()),
            Self::Blob(d) => String::from_utf8(d.clone()).ok(),
            Self::Integer(i) => Some(i.to_string()),
            _ => None,
        }
    }

    pub fn data_value(&self) -> Option<&[u8]> {
        match self {
            Self::Blob(d) => Some(d),
            Self::Text(s) => Some(s.as_bytes()),
            _ => None,
        }
    }

    pub fn int_value(&self) -> Option<i64> {
        match self {
            Self::Integer(i) => Some(*i),
            _ => None,
        }
    }

    /// `.text(value)` or `.null`, like `value.map(SQLValue.text) ?? .null`.
    pub fn optional_text(value: Option<&str>) -> Self {
        value.map_or(Self::Null, |v| Self::Text(v.to_owned()))
    }
}

impl ToSql for SqlValue {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::Borrowed(match self {
            Self::Null => ValueRef::Null,
            Self::Integer(v) => ValueRef::Integer(*v),
            Self::Real(v) => ValueRef::Real(*v),
            Self::Text(v) => ValueRef::Text(v.as_bytes()),
            Self::Blob(v) => ValueRef::Blob(v),
        }))
    }
}

impl From<ValueRef<'_>> for SqlValue {
    fn from(value: ValueRef<'_>) -> Self {
        match value {
            ValueRef::Null => Self::Null,
            ValueRef::Integer(v) => Self::Integer(v),
            ValueRef::Real(v) => Self::Real(v),
            ValueRef::Text(v) => Self::Text(String::from_utf8_lossy(v).into_owned()),
            ValueRef::Blob(v) => Self::Blob(v.to_vec()),
        }
    }
}

fn step_error(error: rusqlite::Error) -> DatabaseError {
    DatabaseError::Step(error.to_string())
}

/// Runs statements on a connection. [`SqliteDatabase`] hands one to the body of a
/// transaction; its own `execute`/`run`/`query` lock the connection and use one too.
pub struct Executor<'a> {
    connection: &'a Connection,
}

impl Executor<'_> {
    /// Runs one or more statements without bindings (`sqlite3_exec`).
    pub fn execute(&self, sql: &str) -> Result<(), DatabaseError> {
        self.connection.execute_batch(sql).map_err(step_error)
    }

    /// Runs one statement, stepping it once; a returned row is not an error.
    pub fn run(&self, sql: &str, bindings: &[SqlValue]) -> Result<(), DatabaseError> {
        let mut statement = self.prepare(sql, bindings)?;
        let mut rows = statement.raw_query();
        rows.next().map_err(step_error)?;
        Ok(())
    }

    pub fn query(&self, sql: &str, bindings: &[SqlValue]) -> Result<Vec<Vec<SqlValue>>, DatabaseError> {
        let mut statement = self.prepare(sql, bindings)?;
        let columns = statement.column_count();
        let mut rows = statement.raw_query();
        let mut result = Vec::new();
        while let Some(row) = rows.next().map_err(step_error)? {
            let mut values = Vec::with_capacity(columns);
            for index in 0..columns {
                values.push(SqlValue::from(row.get_ref(index).map_err(step_error)?));
            }
            result.push(values);
        }
        Ok(result)
    }

    pub fn user_version(&self) -> Result<i64, DatabaseError> {
        Ok(self
            .query("PRAGMA user_version", &[])?
            .first()
            .and_then(|row| row.first())
            .and_then(SqlValue::int_value)
            .unwrap_or(0))
    }

    fn prepare(&self, sql: &str, bindings: &[SqlValue]) -> Result<rusqlite::Statement<'_>, DatabaseError> {
        let prepare_error = |error: rusqlite::Error| DatabaseError::Prepare {
            message: error.to_string(),
            sql: sql.to_owned(),
        };
        let mut statement = self.connection.prepare(sql).map_err(prepare_error)?;
        for (index, value) in bindings.iter().enumerate() {
            statement.raw_bind_parameter(index + 1, value).map_err(prepare_error)?;
        }
        Ok(statement)
    }
}

/// A small, serialized SQLite wrapper. All access goes through one connection
/// guarded by a lock; WAL mode and `synchronous=FULL` favour durability of release
/// state over write throughput.
pub struct SqliteDatabase {
    connection: Mutex<Connection>,
    path: Option<PathBuf>,
}

impl SqliteDatabase {
    /// Opens (creating if needed) a database file, or an in-memory database when `path` is None.
    pub fn new(path: Option<&Path>) -> Result<Self, DatabaseError> {
        let flags =
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE | OpenFlags::SQLITE_OPEN_FULL_MUTEX;
        let connection = match path {
            Some(path) => Connection::open_with_flags(path, flags),
            None => Connection::open_in_memory_with_flags(flags),
        }
        .map_err(|e| DatabaseError::Open(e.to_string()))?;
        let database = Self {
            connection: Mutex::new(connection),
            path: path.map(Path::to_path_buf),
        };
        if let Some(path) = path {
            set_private_permissions(path);
            database.execute("PRAGMA journal_mode=WAL")?;
        }
        database.execute("PRAGMA foreign_keys=ON")?;
        database.execute("PRAGMA synchronous=FULL")?;
        // Swift ignores the result of `sqlite3_busy_timeout`.
        let _ = database.lock().busy_timeout(Duration::from_millis(5000));
        Ok(database)
    }

    /// The database file, or None for an in-memory database (Swift's `url`).
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    fn lock(&self) -> MutexGuard<'_, Connection> {
        self.connection.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn execute(&self, sql: &str) -> Result<(), DatabaseError> {
        Executor {
            connection: &self.lock(),
        }
        .execute(sql)
    }

    pub fn run(&self, sql: &str, bindings: &[SqlValue]) -> Result<(), DatabaseError> {
        Executor {
            connection: &self.lock(),
        }
        .run(sql, bindings)
    }

    pub fn query(&self, sql: &str, bindings: &[SqlValue]) -> Result<Vec<Vec<SqlValue>>, DatabaseError> {
        Executor {
            connection: &self.lock(),
        }
        .query(sql, bindings)
    }

    /// Runs `body` inside a `BEGIN IMMEDIATE` transaction, rolling back on error (or panic).
    pub fn transaction<T, E: From<DatabaseError>>(
        &self,
        body: impl FnOnce(&Executor<'_>) -> Result<T, E>,
    ) -> Result<T, E> {
        let mut connection = self.lock();
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(step_error)?;
        let value = body(&Executor {
            connection: &transaction,
        })?;
        transaction.commit().map_err(step_error)?;
        Ok(value)
    }

    pub fn user_version(&self) -> Result<i64, DatabaseError> {
        Executor {
            connection: &self.lock(),
        }
        .user_version()
    }
}

/// Owner-only access to the database file (0600). Errors are ignored, as in Swift (`try?`).
#[cfg(unix)]
fn set_private_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

/// Windows has no POSIX modes; the file inherits the user profile's ACL.
#[cfg(not(unix))]
fn set_private_permissions(_path: &Path) {}
