//! Store side of a scan: load a library's file rows, apply a [`Plan`]'s
//! operations in one `IMMEDIATE` transaction with cached prepared
//! statements (atomic: a failed scan writes nothing).
//!
//! Only `files`, `audio_meta`, `analysis` and `libraries.online` are
//! written. User data tables are never touched; rows are never deleted, so
//! user data on missing files survives.

use rusqlite::{params, Connection, TransactionBehavior};

use super::reconcile::{DbFile, Identity, Op};
use crate::Result;

/// All file rows of `library_id`.
pub(crate) fn load_files(conn: &Connection, library_id: i64) -> Result<Vec<DbFile>> {
    let mut stmt = conn.prepare(
        "SELECT id, rel_path, size, mtime, quick_hash, status FROM files WHERE library_id = ?1",
    )?;
    let rows = stmt.query_map([library_id], |r| {
        Ok(DbFile {
            id: r.get(0)?,
            rel_path: r.get(1)?,
            size: r.get(2)?,
            mtime: r.get(3)?,
            quick_hash: r.get(4)?,
            status: r.get(5)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Status a row returns to when its unchanged file is present again:
/// derived from the stage rows that survived while it was missing. Never
/// `error`, so a stale `error` text (e.g. an analysis failure recorded
/// before the file went missing) is cleared with it; the pipeline retries.
const RESTORE_SQL: &str = "UPDATE files SET error = NULL, status = CASE \
       WHEN EXISTS (SELECT 1 FROM analysis WHERE file_id = files.id) THEN 'analyzed' \
       WHEN EXISTS (SELECT 1 FROM audio_meta WHERE file_id = files.id) THEN 'meta' \
       ELSE 'new' END \
     WHERE id = ?1";

/// Applies `ops` for `library_id` and marks the library online.
pub(crate) fn apply(conn: &mut Connection, library_id: i64, ops: &[Op]) -> Result<()> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    tx.execute(
        "UPDATE libraries SET online = 1 WHERE id = ?1",
        [library_id],
    )?;
    for op in ops {
        match op {
            Op::Insert {
                rel_path,
                size,
                mtime,
                identity,
            } => {
                let (hash, status, error) = identity_columns(identity);
                tx.prepare_cached(
                    "INSERT INTO files (library_id, rel_path, size, mtime, quick_hash, status, error) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                )?
                .execute(params![library_id, rel_path, size, mtime, hash, status, error])?;
            }
            Op::Update {
                id,
                size,
                mtime,
                identity,
            } => {
                let (hash, status, error) = identity_columns(identity);
                tx.prepare_cached(
                    "UPDATE files SET size = ?2, mtime = ?3, quick_hash = ?4, status = ?5, \
                     error = ?6, content_hash = NULL, analysis_version = 0 WHERE id = ?1",
                )?
                .execute(params![id, size, mtime, hash, status, error])?;
                // Stage results describe the old content.
                tx.prepare_cached("DELETE FROM audio_meta WHERE file_id = ?1")?
                    .execute([id])?;
                tx.prepare_cached("DELETE FROM analysis WHERE file_id = ?1")?
                    .execute([id])?;
            }
            Op::Restore { id } => {
                tx.prepare_cached(RESTORE_SQL)?.execute([id])?;
            }
            Op::Move {
                id,
                rel_path,
                mtime,
                restore,
            } => {
                tx.prepare_cached("UPDATE files SET rel_path = ?2, mtime = ?3 WHERE id = ?1")?
                    .execute(params![id, rel_path, mtime])?;
                if *restore {
                    tx.prepare_cached(RESTORE_SQL)?.execute([id])?;
                }
            }
            Op::Missing { id } => {
                tx.prepare_cached("UPDATE files SET status = 'missing' WHERE id = ?1")?
                    .execute([id])?;
            }
        }
    }
    tx.commit()?;
    Ok(())
}

/// `(quick_hash, status, error)` columns for an identity.
fn identity_columns(identity: &Identity) -> (Option<i64>, &'static str, Option<&str>) {
    match identity {
        Identity::Hash(h) => (Some(*h), "new", None),
        Identity::Error(msg) => (None, "error", Some(msg.as_str())),
    }
}
