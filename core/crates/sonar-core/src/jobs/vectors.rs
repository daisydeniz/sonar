//! The `dsp` stage's side of the vector store (ADR-023). Only the job
//! coordinator writes vectors (one writer, no scanner involvement):
//!
//! - **Start of the stage** ([`open`]): open the store next to the
//!   database and reconcile it with `file_stages`. A row stays live while
//!   its file has an `ok`/`partial` `dsp` row (any version, any file
//!   status: missing or offline files keep their vectors and are filtered
//!   at query time like in the text index). Rows of files whose `dsp` row
//!   is gone (content changed, file deleted) are tombstoned. Files whose
//!   *current-version* `dsp` row has no vector (store lost or reset, power
//!   loss before the rows reached disk) get that row deleted, so this run
//!   analyzes them again.
//! - **Per batch** ([`append`]): the rows are appended *before* the batch's
//!   SQLite commit. A crash in between leaves the files pending; the re-run
//!   appends again and the newer row wins.
//! - **End of the stage** ([`finish`]): compaction once dead rows reach a
//!   quarter of the store.

use std::collections::{HashMap, HashSet};

use rusqlite::{params, Connection, TransactionBehavior};

use super::queue::Claim;
use super::stage::{FileResult, Outcome, Stage, DSP_VERSION};
use crate::dsp::aspects::AspectVectors;
use crate::index::vectors::{self, VectorStore};
use crate::{Error, Result, Store};

/// Compact when dead rows ≥ rows / this.
const COMPACT_DEAD_FRACTION: u64 = 4;

/// Opens the vector store of `store`'s database and reconciles it.
///
/// # Errors
/// [`Error::Internal`] for an in-memory store, vector store and SQLite
/// errors.
pub(super) fn open(store: &mut Store) -> Result<VectorStore> {
    let db = store.path().ok_or_else(|| Error::Internal {
        reason: "the job pipeline needs a file-backed store".into(),
    })?;
    let (mut vs, state) = VectorStore::open(vectors::dir_for_db(db))?;
    if state != vectors::OpenState::Ready {
        tracing::info!(?state, "vector store opened");
    }
    reconcile(store.conn_mut(), &mut vs)?;
    Ok(vs)
}

/// Tombstones rows without a `dsp` result and re-queues current results
/// without a row (see the module docs). Returns `(tombstoned, requeued)`.
pub(super) fn reconcile(conn: &mut Connection, vs: &mut VectorStore) -> Result<(u64, u64)> {
    let done: HashMap<i64, u32> = conn
        .prepare(
            "SELECT file_id, version FROM file_stages \
             WHERE stage = ?1 AND outcome IN (?2, ?3)",
        )?
        .query_map(
            params![
                Stage::Dsp.as_str(),
                Outcome::Ok.as_str(),
                Outcome::Partial.as_str()
            ],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?
        .collect::<rusqlite::Result<_>>()?;
    let keep: HashSet<i64> = done.keys().copied().collect();
    let tombstoned = vs.retain(&keep)?;
    let lost: Vec<i64> = done
        .iter()
        .filter(|&(&id, &v)| v == DSP_VERSION && vs.row_of(id).is_none())
        .map(|(&id, _)| id)
        .collect();
    if !lost.is_empty() {
        tracing::warn!(files = lost.len(), "analysis without vectors, re-queued");
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        {
            let mut del =
                tx.prepare_cached("DELETE FROM file_stages WHERE file_id = ?1 AND stage = ?2")?;
            for id in &lost {
                del.execute(params![id, Stage::Dsp.as_str()])?;
            }
        }
        tx.commit()?;
    }
    Ok((tombstoned, lost.len() as u64))
}

/// Appends the vectors of every completed file in `results`.
pub(super) fn append(vs: &mut VectorStore, results: &[(Claim, FileResult)]) -> Result<()> {
    let batch: Vec<(i64, &AspectVectors)> = results
        .iter()
        .filter_map(|(claim, r)| match r {
            FileResult::Done {
                analysis: Some(a), ..
            } => Some((claim.id, &a.aspects)),
            _ => None,
        })
        .collect();
    vs.append(&batch)
}

/// Compacts when enough rows are dead.
pub(super) fn finish(vs: &mut VectorStore) -> Result<()> {
    let dead = vs.rows() - vs.live_rows();
    if dead > 0 && dead * COMPACT_DEAD_FRACTION >= vs.rows() {
        let r = vs.compact()?;
        tracing::info!(
            before = r.rows_before,
            after = r.rows_after,
            secs = r.seconds,
            "vector store compacted"
        );
    }
    Ok(())
}
