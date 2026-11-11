//! Database side of the pipeline: which files are pending, claiming them in
//! batches, and committing a whole batch of results in one transaction.
//!
//! Atomicity: a batch's `audio_meta`, `analysis` (kind/tempo/key/pitch
//! columns of the `dsp` stage), `file_stages` and `files` writes share
//! one `IMMEDIATE` transaction, so a crash (kill -9) leaves either the whole
//! batch or nothing. `file_stages` has `PRIMARY KEY (file_id, stage)` and
//! the pending query keys off it, so re-running after a crash cannot
//! produce duplicates.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};

use super::stage::{FileResult, Outcome, Stage};
use crate::store::{analysis as store_analysis, audio_meta, is_busy, key_pitch};
use crate::Result;

/// A file taken from the queue, with the identity it had at claim time.
#[derive(Debug, Clone)]
pub(crate) struct Claim {
    pub id: i64,
    pub path: PathBuf,
    pub size: i64,
    pub mtime: i64,
}

/// Counts from one committed batch.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CommitCounts {
    pub ok: u64,
    pub partial: u64,
    pub errors: u64,
    /// Left pending: I/O error or the file changed since the claim.
    pub skipped: u64,
}

/// `FROM` part of the pending queries (a macro so `concat!` can build
/// constant SQL strings).
macro_rules! pending_from {
    () => {
        "FROM files f JOIN libraries l ON l.id = f.library_id \
         LEFT JOIN file_stages s ON s.file_id = f.id AND s.stage = ?1"
    };
}

/// Stage eligibility of a file row `f` against its stage row `$s` (a table
/// alias, possibly NULL from a `LEFT JOIN`) for the stage version `$v` (an SQL
/// expression). Shared by the queue and `sonar stats` so both agree on
/// "pending". Excludes the library's `online` flag and the id cursor, which
/// only the queue applies.
macro_rules! eligible {
    ($s:literal, $v:literal) => {
        concat!(
            "((+f.status IN ('new', 'meta', 'analyzed') \
              OR (+f.status = 'error' AND ",
            $s,
            ".outcome = 'error')) \
             AND (",
            $s,
            ".file_id IS NULL OR ",
            $s,
            ".version < ",
            $v,
            "))"
        )
    };
}
pub(crate) use eligible;

/// Files eligible for a stage: online library, a status the pipeline works
/// on (`missing` and scan-level `error` rows never), and no row for the
/// stage or one older than its version. A file with `status = 'error'` is
/// eligible only for the stage whose *own* `error` row is outdated (a
/// version bump of that stage retries it; bumping another stage does not).
///
/// The unary `+` on `f.status` keeps SQLite off the `files_status` index:
/// with it the planner walks the primary key in id order (`id > ?2`), so a
/// batch costs O(batch) with no sort, instead of scanning and sorting all
/// remaining rows per batch.
macro_rules! pending_where {
    () => {
        concat!("l.online = 1 AND f.id > ?2 AND ", eligible!("s", "?3"))
    };
}

const COUNT_SQL: &str = concat!(
    "SELECT count(*) ",
    pending_from!(),
    " WHERE ",
    pending_where!()
);

const CLAIM_SQL: &str = concat!(
    "SELECT f.id, l.root_path, f.rel_path, f.size, f.mtime ",
    pending_from!(),
    " WHERE ",
    pending_where!(),
    " ORDER BY f.id LIMIT ?4"
);

/// Number of files pending for `stage`.
pub(crate) fn count_pending(conn: &Connection, stage: Stage) -> Result<u64> {
    let n: i64 = conn
        .prepare_cached(COUNT_SQL)?
        .query_row(params![stage.as_str(), 0i64, stage.version()], |r| r.get(0))?;
    Ok(u64::try_from(n).unwrap_or(0))
}

/// Up to `limit` pending files with `id > after_id`, ascending by id.
pub(crate) fn claim_batch(
    conn: &Connection,
    stage: Stage,
    after_id: i64,
    limit: usize,
) -> Result<Vec<Claim>> {
    let limit = i64::try_from(limit).unwrap_or(i64::MAX);
    let mut stmt = conn.prepare_cached(CLAIM_SQL)?;
    let rows = stmt.query_map(
        params![stage.as_str(), after_id, stage.version(), limit],
        |r| {
            let root: String = r.get(1)?;
            let rel: String = r.get(2)?;
            Ok(Claim {
                id: r.get(0)?,
                path: Path::new(&root).join(rel),
                size: r.get(3)?,
                mtime: r.get(4)?,
            })
        },
    )?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// True if the row still has the identity captured at claim time and was
/// not marked missing since (a scan may have run meanwhile).
fn still_current(conn: &Connection, c: &Claim) -> Result<bool> {
    let row: Option<(i64, i64, String)> = conn
        .prepare_cached("SELECT size, mtime, status FROM files WHERE id = ?1")?
        .query_row([c.id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .optional()?;
    Ok(matches!(row, Some((size, mtime, status))
        if size == c.size && mtime == c.mtime && status != "missing"))
}

fn put_stage(
    conn: &Connection,
    id: i64,
    stage: Stage,
    outcome: Outcome,
    message: Option<&str>,
    now: i64,
) -> Result<()> {
    conn.prepare_cached(
        "INSERT OR REPLACE INTO file_stages (file_id, stage, version, outcome, message, done_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )?
    .execute(params![
        id,
        stage.as_str(),
        stage.version(),
        outcome.as_str(),
        message,
        now
    ])?;
    Ok(())
}

/// [`commit`], retried with short backoff while SQLite reports BUSY/LOCKED
/// (each attempt already waits the connection's busy timeout), until
/// `patience` has passed or `give_up()` is true. A failed attempt rolls
/// back and the results stay in memory, so retrying is safe.
pub(crate) fn commit_retrying(
    conn: &mut Connection,
    stage: Stage,
    results: &[(Claim, FileResult)],
    patience: Duration,
    give_up: impl Fn() -> bool,
) -> Result<CommitCounts> {
    let deadline = Instant::now() + patience;
    let mut pause = Duration::from_millis(5);
    loop {
        match commit(conn, stage, results) {
            Err(e) if is_busy(&e) && !give_up() && Instant::now() < deadline => {
                std::thread::sleep(pause);
                pause = (pause * 2).min(Duration::from_millis(200));
            }
            other => return other,
        }
    }
}

/// Applies `results` for `stage` in one transaction. Results of files that
/// changed since the claim are dropped silently (they are pending again
/// after the scan reset them). Returns the per-outcome counts.
pub(crate) fn commit(
    conn: &mut Connection,
    stage: Stage,
    results: &[(Claim, FileResult)],
) -> Result<CommitCounts> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let now = now_secs();
    let mut counts = CommitCounts::default();
    for (claim, result) in results {
        match result {
            FileResult::Unstarted => {}
            FileResult::Skipped => counts.skipped += 1,
            _ if !still_current(&tx, claim)? => counts.skipped += 1,
            FileResult::Done { meta, analysis } => {
                let outcome = if meta.partial {
                    Outcome::Partial
                } else {
                    Outcome::Ok
                };
                let message = meta
                    .problem
                    .as_deref()
                    .or_else(|| meta.partial.then_some("partial decode"));
                let status = match stage {
                    Stage::Meta => {
                        audio_meta::upsert_format(&tx, claim.id, meta)?;
                        "meta"
                    }
                    _ => {
                        audio_meta::upsert(&tx, claim.id, meta)?;
                        if let Some(a) = analysis {
                            store_analysis::upsert_kind_tempo(&tx, claim.id, &a.kind_tempo)?;
                            key_pitch::upsert_key_pitch(&tx, claim.id, &a.key_pitch)?;
                        }
                        "analyzed"
                    }
                };
                put_stage(&tx, claim.id, stage, outcome, message, now)?;
                // `meta` never downgrades a file the DSP stage already
                // finished (re-run after a version bump).
                tx.prepare_cached(
                    "UPDATE files SET status = ?2, error = NULL \
                     WHERE id = ?1 AND (?2 = 'analyzed' OR status IN ('new', 'error'))",
                )?
                .execute(params![claim.id, status])?;
                match outcome {
                    Outcome::Partial => counts.partial += 1,
                    _ => counts.ok += 1,
                }
            }
            FileResult::Failed(message) => {
                put_stage(&tx, claim.id, stage, Outcome::Error, Some(message), now)?;
                tx.prepare_cached("UPDATE files SET status = 'error', error = ?2 WHERE id = ?1")?
                    .execute(params![claim.id, message])?;
                counts.errors += 1;
            }
        }
    }
    tx.commit()?;
    Ok(counts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;

    #[test]
    fn claim_walks_the_primary_key_without_sorting_or_status_index() {
        let store = Store::open_in_memory().expect("store");
        let conn = store.conn();
        let mut stmt = conn
            .prepare(&format!("EXPLAIN QUERY PLAN {CLAIM_SQL}"))
            .expect("prepare");
        let plan: Vec<String> = stmt
            .query_map(params!["meta", 0i64, 1u32, 10i64], |r| {
                r.get::<_, String>(3)
            })
            .expect("plan")
            .collect::<rusqlite::Result<_>>()
            .expect("rows");
        let plan = plan.join(" | ");
        assert!(!plan.contains("TEMP B-TREE"), "sorts every batch: {plan}");
        assert!(
            !plan.contains("files_status"),
            "uses the status index: {plan}"
        );
        assert!(plan.contains("INTEGER PRIMARY KEY"), "{plan}");
    }

    /// File database plus a writer connection with a short busy timeout.
    fn two_connections() -> (tempfile::TempDir, Connection, Connection) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("sonar.db");
        drop(Store::open(&path).expect("store"));
        let open = || {
            let c = Connection::open(&path).expect("open");
            c.busy_timeout(Duration::from_millis(20)).expect("timeout");
            c
        };
        (tmp, open(), open())
    }

    #[test]
    fn commit_retries_past_the_busy_timeout_until_the_lock_is_free() {
        let (_tmp, mut writer, holder) = two_connections();
        holder.execute_batch("BEGIN IMMEDIATE").expect("lock");
        // Plain commit gives up after the 20 ms busy timeout.
        let err = commit(&mut writer, Stage::Meta, &[]).expect_err("busy");
        assert!(is_busy(&err));

        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            holder.execute_batch("COMMIT").expect("unlock");
        });
        let counts = commit_retrying(
            &mut writer,
            Stage::Meta,
            &[],
            Duration::from_secs(30),
            || false,
        )
        .expect("retried until free");
        assert_eq!(counts, CommitCounts::default());
        release.join().expect("join");
    }

    #[test]
    fn commit_retry_stops_at_patience_or_when_asked() {
        let (_tmp, mut writer, holder) = two_connections();
        holder.execute_batch("BEGIN IMMEDIATE").expect("lock");
        let start = Instant::now();
        let err = commit_retrying(
            &mut writer,
            Stage::Meta,
            &[],
            Duration::from_millis(150),
            || false,
        )
        .expect_err("patience exhausted");
        assert!(is_busy(&err) && start.elapsed() >= Duration::from_millis(150));
        let err = commit_retrying(
            &mut writer,
            Stage::Meta,
            &[],
            Duration::from_secs(30),
            || true,
        )
        .expect_err("gave up");
        assert!(is_busy(&err));
    }
}
