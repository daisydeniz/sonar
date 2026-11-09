//! SQLite store (docs/ARCHITECTURE.md §3): connection setup and schema.
//!
//! Every file-backed connection runs in WAL mode with foreign keys on,
//! `synchronous=NORMAL` (durable across app crashes; a power loss may drop
//! the last commits, never corrupt) and a busy timeout so concurrent readers
//! and the writer wait instead of failing. Opening migrates the schema to
//! [`LATEST_VERSION`]; a database from a newer build is refused before
//! anything is written to it.

mod migrations;
#[cfg(test)]
mod tests;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rusqlite::Connection;

use crate::{paths, Error, Result};

pub use migrations::LATEST_VERSION;

/// How long a connection waits for a lock held by another connection.
pub const BUSY_TIMEOUT: Duration = Duration::from_millis(5000);

/// An open, migrated Sonar database.
#[derive(Debug)]
pub struct Store {
    conn: Connection,
    path: Option<PathBuf>,
}

/// Snapshot of store health, for `sonar store info` and diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreInfo {
    /// Database file, `None` for in-memory stores.
    pub path: Option<PathBuf>,
    /// `PRAGMA user_version` (schema version).
    pub schema_version: i64,
    /// `PRAGMA journal_mode` (`wal` for file databases).
    pub journal_mode: String,
    /// Row count of every table, sorted by table name.
    pub tables: Vec<TableCount>,
}

/// Number of rows in one table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableCount {
    /// Table name.
    pub name: String,
    /// Row count.
    pub rows: i64,
}

impl Store {
    /// Opens (creating if missing, including parent directories) the
    /// database at `path`, enables WAL and migrates it to the latest schema.
    ///
    /// # Errors
    /// [`Error::Io`] if the parent directory cannot be created,
    /// [`Error::UnsupportedSchema`] if the file was written by a newer build
    /// (the file is left untouched), [`Error::Pragma`] (no WAL / no FKs),
    /// [`Error::Migration`], or [`Error::Sqlite`] (e.g. not a database).
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
        }
        let conn = Connection::open(path)?;
        Self::init(conn, Some(path.to_path_buf()))
    }

    /// Opens the store at [`paths::default_db_path`] (`SONAR_HOME` or the
    /// macOS Application Support directory), creating it if needed.
    ///
    /// # Errors
    /// [`Error::NoDataDir`], plus everything [`Store::open`] returns.
    pub fn open_default() -> Result<Self> {
        Self::open(paths::default_db_path()?)
    }

    /// Opens a fresh, migrated in-memory database (tests, benchmarks).
    ///
    /// WAL does not apply to in-memory databases; their journal mode is
    /// `memory`. All other pragmas match [`Store::open`].
    ///
    /// # Errors
    /// [`Error::Sqlite`] or [`Error::Migration`].
    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?, None)
    }

    /// Applies connection pragmas and migrations. Order matters: the schema
    /// version is checked before the first write (WAL switch) so a newer
    /// database is never modified; migrations run with foreign keys OFF
    /// (see [`migrations`]) and they are switched ON, verified, afterwards.
    fn init(mut conn: Connection, path: Option<PathBuf>) -> Result<Self> {
        conn.busy_timeout(BUSY_TIMEOUT)?;
        let file_backed = path.is_some();
        retry_busy(|| Self::prepare(&mut conn, file_backed))?;
        conn.pragma_update(None, "foreign_keys", true)?;
        let fk: i64 = conn.pragma_query_value(None, "foreign_keys", |row| row.get(0))?;
        if fk != 1 {
            return Err(Error::Pragma {
                name: "foreign_keys",
                expected: "1",
                found: fk.to_string(),
            });
        }
        Ok(Self { conn, path })
    }

    /// Version check, WAL switch, `synchronous`, migrations. Idempotent, so
    /// [`retry_busy`] may run it again after a lock conflict.
    fn prepare(conn: &mut Connection, file_backed: bool) -> Result<()> {
        migrations::ensure_supported(migrations::user_version(conn)?, LATEST_VERSION)?;
        if file_backed {
            let mode: String =
                conn.pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get(0))?;
            if !mode.eq_ignore_ascii_case("wal") {
                return Err(Error::Pragma {
                    name: "journal_mode",
                    expected: "wal",
                    found: mode,
                });
            }
        }
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        migrations::migrate(conn, migrations::MIGRATIONS)?;
        Ok(())
    }

    /// Database file, `None` for in-memory stores.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Current schema version (`PRAGMA user_version`).
    ///
    /// # Errors
    /// [`Error::Sqlite`].
    pub fn schema_version(&self) -> Result<i64> {
        migrations::user_version(&self.conn)
    }

    /// Current journal mode, lowercase (`wal` for file databases).
    ///
    /// # Errors
    /// [`Error::Sqlite`].
    pub fn journal_mode(&self) -> Result<String> {
        let mode: String = self
            .conn
            .pragma_query_value(None, "journal_mode", |row| row.get(0))?;
        Ok(mode.to_ascii_lowercase())
    }

    /// Row counts of every (non-internal) table, sorted by name.
    ///
    /// # Errors
    /// [`Error::Sqlite`].
    pub fn table_counts(&self) -> Result<Vec<TableCount>> {
        let conn = self.conn();
        let names = {
            let mut stmt = conn.prepare(
                "SELECT name FROM sqlite_schema \
                 WHERE type = 'table' AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\' \
                 ORDER BY name",
            )?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        names
            .into_iter()
            .map(|name| {
                let sql = format!("SELECT count(*) FROM {}", quote_ident(&name));
                let rows = conn.query_row(&sql, [], |row| row.get(0))?;
                Ok(TableCount { name, rows })
            })
            .collect()
    }

    /// Path, schema version, journal mode and row counts in one call.
    ///
    /// # Errors
    /// [`Error::Sqlite`].
    pub fn info(&self) -> Result<StoreInfo> {
        Ok(StoreInfo {
            path: self.path.clone(),
            schema_version: self.schema_version()?,
            journal_mode: self.journal_mode()?,
            tables: self.table_counts()?,
        })
    }

    /// Underlying connection, for other core modules (scan, jobs, index).
    pub(crate) fn conn(&self) -> &Connection {
        &self.conn
    }
}

/// Longest pause between [`retry_busy`] attempts.
const MAX_RETRY_PAUSE: Duration = Duration::from_millis(50);

/// Runs `op`, retrying with exponential backoff while SQLite reports
/// BUSY/LOCKED. New attempts start only within [`BUSY_TIMEOUT`]; since one
/// attempt may itself block up to `BUSY_TIMEOUT` in SQLite's busy handler,
/// the worst case is about 2 × `BUSY_TIMEOUT`.
///
/// `busy_timeout` alone is not enough at open time: some lock conflicts
/// (notably switching a new file to WAL while another connection opens it)
/// return `SQLITE_BUSY` without invoking the busy handler
/// (<https://www.sqlite.org/c3ref/busy_handler.html>). `op` must be
/// idempotent.
fn retry_busy<T>(mut op: impl FnMut() -> Result<T>) -> Result<T> {
    let deadline = Instant::now() + BUSY_TIMEOUT;
    let mut pause = Duration::from_millis(1);
    loop {
        match op() {
            Err(e) if is_busy(&e) && Instant::now() < deadline => {
                std::thread::sleep(pause);
                pause = (pause * 2).min(MAX_RETRY_PAUSE);
            }
            other => return other,
        }
    }
}

/// True for SQLite lock conflicts (`SQLITE_BUSY`, `SQLITE_LOCKED`).
fn is_busy(err: &Error) -> bool {
    let sqlite = match err {
        Error::Sqlite(e) | Error::Migration { source: e, .. } => e,
        _ => return false,
    };
    matches!(
        sqlite.sqlite_error_code(),
        Some(rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked)
    )
}

/// Quotes `name` as an SQL identifier (`"name"`, inner quotes doubled), for
/// the few places a table name cannot be a bound parameter.
pub(crate) fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}
