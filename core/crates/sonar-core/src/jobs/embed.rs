//! The `embed` stage (T5.2, ADR-027): per-file work (decode + front end on a
//! rayon worker, the model run on the session pool) and the stage's side of
//! the embedding store, which mirrors what `vectors.rs` does for the aspect
//! vectors:
//!
//! - **Start of the stage** ([`open`]): open `<db dir>/index/clap` and
//!   delete the current-version `ok`/`partial` `embed` rows whose file has no
//!   embedding (store lost, reset, or power loss before the rows reached
//!   disk), so this run embeds those files again.
//! - **Per batch** ([`append`]): the rows are appended *before* the batch's
//!   SQLite commit. A crash in between leaves the files pending; the re-run
//!   appends again and the newer row wins, so there are no duplicates.

use std::cell::RefCell;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use rusqlite::{params, Connection, TransactionBehavior};

use super::queue::Claim;
use super::stage::{FileResult, Outcome, Stage, EMBED_VERSION};
use crate::embed::store::{self, EmbedStore, OpenState};
use crate::embed::{AudioPrep, EmbedPool, Embedding};
use crate::{quiet_panic, Error, Result, Store};

thread_local! {
    static PREP: RefCell<AudioPrep> = RefCell::new(AudioPrep::new());
}

/// Opens the embedding store of `store`'s database and reconciles it.
///
/// # Errors
/// [`Error::Internal`] for an in-memory store, store and SQLite errors.
pub(super) fn open(store: &mut Store) -> Result<EmbedStore> {
    let db = store.path().ok_or_else(|| Error::Internal {
        reason: "the job pipeline needs a file-backed store".into(),
    })?;
    let (es, state) = EmbedStore::open(store::dir_for_db(db))?;
    if state != OpenState::Ready {
        tracing::info!(?state, "embedding store opened");
    }
    let requeued = reconcile(store.conn_mut(), &es)?;
    if requeued > 0 {
        tracing::warn!(
            files = requeued,
            "embedded files without a vector, re-queued"
        );
    }
    Ok(es)
}

/// Re-queues current `embed` results that have no stored embedding. Returns
/// how many.
pub(super) fn reconcile(conn: &mut Connection, es: &EmbedStore) -> Result<u64> {
    let lost: Vec<i64> = conn
        .prepare(
            "SELECT file_id FROM file_stages \
             WHERE stage = ?1 AND version = ?2 AND outcome IN (?3, ?4)",
        )?
        .query_map(
            params![
                Stage::Embed.as_str(),
                EMBED_VERSION,
                Outcome::Ok.as_str(),
                Outcome::Partial.as_str()
            ],
            |r| r.get::<_, i64>(0),
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?
        .into_iter()
        .filter(|&id| !es.contains(id))
        .collect();
    if !lost.is_empty() {
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        {
            let mut del =
                tx.prepare_cached("DELETE FROM file_stages WHERE file_id = ?1 AND stage = ?2")?;
            for id in &lost {
                del.execute(params![id, Stage::Embed.as_str()])?;
            }
        }
        tx.commit()?;
    }
    Ok(lost.len() as u64)
}

/// Appends the embeddings of every completed file in `results`.
pub(super) fn append(es: &mut EmbedStore, results: &[(Claim, FileResult)]) -> Result<()> {
    let batch: Vec<(i64, &Embedding)> = results
        .iter()
        .filter_map(|(claim, r)| match r {
            FileResult::Embedded { embedding, .. } => Some((claim.id, &**embedding)),
            _ => None,
        })
        .collect();
    es.append(&batch)
}

/// Embeds one file: decode (native rate -> 48 kHz, first 10 s), front end,
/// model run on `pool`. Never panics.
///
/// - Unreadable now (I/O, cancelled): [`FileResult::Skipped`], stays pending.
/// - Not audio, no frames, damaged beyond use: [`FileResult::Failed`].
/// - The pool failing is not the file's fault: `Skipped` too.
pub(super) fn embed_file(pool: &EmbedPool, path: &Path, cancel: Arc<AtomicBool>) -> FileResult {
    let prepared =
        quiet_panic::guarded(|| PREP.with(|p| p.borrow_mut().prepare(path, Some(cancel))))
            .unwrap_or_else(|msg| {
                PREP.with(|p| p.borrow_mut().reset());
                Err(Error::Decode {
                    path: path.to_path_buf(),
                    reason: format!("internal error: {msg}"),
                })
            });
    let prepared = match prepared {
        Ok(p) => p,
        Err(Error::Io { path, source }) => {
            tracing::debug!(path = %path.display(), error = %source, "i/o error, file left pending");
            return FileResult::Skipped;
        }
        Err(Error::Cancelled) => return FileResult::Skipped,
        Err(e) => return FileResult::Failed(e.to_string()),
    };
    match pool.embed(prepared.features) {
        Ok(e) => FileResult::Embedded {
            partial: prepared.partial,
            problem: prepared.problem,
            embedding: Box::new(e),
        },
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "embedding failed, file left pending");
            FileResult::Skipped
        }
    }
}
