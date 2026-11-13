//! One stage's batch loop: batches are claimed ahead and processed on the
//! rayon pool while the coordinator (the only thread that touches the
//! database) commits the finished one.
//!
//! Invariants (ADR-013): every batch is one all-or-nothing transaction,
//! committed in claim order; the id cursor only moves forward while batches
//! are in flight, so no file is claimed twice; claim-ahead stays inside the
//! current stage (the runner starts the next stage only after this one has
//! drained). On pause/cancel workers stop starting files and report them
//! `Unstarted`; the coordinator still commits what finished and keeps the
//! unstarted claims in a retry list. Once the pipeline drained and the job
//! may run again it dispatches them (in batch-size chunks, before claiming
//! anything new); on cancel they are dropped. Unstarted files are not a
//! suffix of the id range (rayon splits a batch across threads), which is
//! why the cursor is never rewound: that would re-offer files already
//! committed.
//!
//! Rayon work-stealing can let a worker blocked in batch N pick up batch
//! N+1 work, which delays N's commit; this costs latency only, not
//! correctness.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rayon::iter::{IntoParallelIterator, ParallelIterator};

use rusqlite::Connection;

use super::embed::CategoryRefiner;
use super::progress::{JobEvent, Shared, StageSummary, StageTiming};
use super::queue::{self, Claim};
use super::stage::{FileResult, Stage};
use super::work::{self, WorkOptions};
use super::JobOptions;
use crate::embed::EmbedStore;
use crate::index::vectors::VectorStore;
use crate::{Error, Result, Store};

/// Where a stage's side output (not in SQLite) goes; appended to before each
/// batch commit (a crash in between only re-runs the batch).
pub(super) enum Sink<'a> {
    /// `dsp`: raw aspect vectors.
    Aspects(&'a mut VectorStore),
    /// `embed`: CLAP embeddings, plus the category refinement (ADR-028)
    /// when the prompt bank is available.
    Embeddings(&'a mut EmbedStore, Option<&'a CategoryRefiner>),
}

impl Sink<'_> {
    fn append(&mut self, conn: &Connection, results: &mut [(Claim, FileResult)]) -> Result<()> {
        match self {
            Self::Aspects(vs) => super::vectors::append(vs, results),
            Self::Embeddings(es, refiner) => {
                super::embed::append(es, results)?;
                match refiner {
                    Some(r) => r.refine(conn, results),
                    None => Ok(()),
                }
            }
        }
    }
}

/// How long a batch commit keeps retrying while another writer (e.g. a long
/// scan) holds the database lock, before the job fails.
const COMMIT_PATIENCE: Duration = Duration::from_secs(60);

/// Batches claimed and in flight at once (the one being committed is already
/// out of the queue): two keeps the pool busy during a commit and fills the
/// tail of one batch with the head of the next.
const LOOKAHEAD: usize = 2;

/// Finished batch handed from the pool to the coordinator.
struct BatchOut {
    results: Vec<(Claim, FileResult)>,
    busy_ns: u64,
    slowest_ns: u64,
}

/// Starts `claims` on the pool; the result arrives on the returned channel.
fn spawn_batch(
    pool: &rayon::ThreadPool,
    stage: Stage,
    claims: Vec<Claim>,
    shared: &Arc<Shared>,
    work: &Arc<WorkOptions>,
) -> Receiver<BatchOut> {
    let (tx, rx) = mpsc::channel();
    let (shared, work) = (Arc::clone(shared), Arc::clone(work));
    pool.spawn(move || {
        let (busy, slowest) = (AtomicU64::new(0), AtomicU64::new(0));
        let results = claims
            .into_par_iter()
            .map(|claim| {
                let t = Instant::now();
                let result = work::process(stage, &claim, &shared, &work);
                let ns = u64::try_from(t.elapsed().as_nanos()).unwrap_or(u64::MAX);
                busy.fetch_add(ns, Ordering::Relaxed);
                slowest.fetch_max(ns, Ordering::Relaxed);
                (claim, result)
            })
            .collect();
        // The receiver is gone only if the coordinator bailed out on an error.
        let _ = tx.send(BatchOut {
            results,
            busy_ns: busy.into_inner(),
            slowest_ns: slowest.into_inner(),
        });
    });
    rx
}

/// Runs one stage over everything pending, in id order. `sink` (the `dsp`
/// and `embed` stages) receives each batch's vectors before its commit.
pub(super) fn run_stage(
    store: &mut Store,
    stage: Stage,
    opts: &JobOptions,
    pool: &rayon::ThreadPool,
    work: &Arc<WorkOptions>,
    shared: &Arc<Shared>,
    mut sink: Option<Sink<'_>>,
) -> Result<StageSummary> {
    let started = Instant::now();
    let total = queue::count_pending(store.conn(), stage)?;
    let mut sum = StageSummary::new(stage, total);
    shared.begin_stage(stage, total);
    shared.emit(JobEvent::StageStarted { stage, total });

    let batch = opts.batch_size.max(1);
    let mut timing = StageTiming {
        threads: pool.current_num_threads(),
        ..StageTiming::default()
    };
    // Cursor over ids: strictly forward-only, so no file is claimed twice
    // and a skipped file is not offered again in this run.
    let mut after = 0i64;
    // Claims whose file never started (pause/cancel hit mid-batch). They are
    // not a suffix of the id range (rayon splits a batch across threads), so
    // they are re-dispatched from here, not by rewinding the cursor.
    let mut retry: Vec<Claim> = Vec::new();
    let mut exhausted = false;
    let mut inflight: VecDeque<Receiver<BatchOut>> = VecDeque::new();
    // On an error: stop workers starting files and let the batches in flight
    // finish, so nothing keeps decoding after the stage returned.
    macro_rules! tri {
        ($e:expr) => {
            match $e {
                Ok(v) => v,
                Err(e) => {
                    shared.abort();
                    for rx in inflight.drain(..) {
                        let _ = rx.recv();
                    }
                    return Err(e);
                }
            }
        };
    }
    loop {
        while (!retry.is_empty() || !exhausted) && inflight.len() < LOOKAHEAD && shared.should_run()
        {
            let claims = if retry.is_empty() {
                let t = Instant::now();
                let claims = tri!(queue::claim_batch(store.conn(), stage, after, batch));
                timing.claim_secs += t.elapsed().as_secs_f64();
                let Some(last) = claims.last() else {
                    exhausted = true;
                    break;
                };
                after = last.id;
                claims
            } else {
                let n = batch.min(retry.len());
                retry.drain(..n).collect()
            };
            inflight.push_back(spawn_batch(pool, stage, claims, shared, work));
        }
        let Some(rx) = inflight.pop_front() else {
            // Drained. Wait out a pause; stop when cancelled or all done.
            if (exhausted && retry.is_empty()) || !shared.wait_runnable() {
                break;
            }
            continue;
        };

        let t = Instant::now();
        let out = tri!(rx.recv().map_err(|_| Error::Internal {
            reason: "worker pool dropped a batch".into(),
        }));
        timing.wait_secs += t.elapsed().as_secs_f64();
        timing.busy_secs += out.busy_ns as f64 * 1e-9;
        timing.max_file_secs += out.slowest_ns as f64 * 1e-9;
        let (unstarted, mut results): (Vec<_>, Vec<_>) = out
            .results
            .into_iter()
            .partition(|(_, r)| matches!(r, FileResult::Unstarted));
        retry.extend(unstarted.into_iter().map(|(c, _)| c));
        if results.is_empty() {
            continue;
        }

        let t = Instant::now();
        // Vectors first: a crash before the commit only re-runs the batch.
        if let Some(sink) = sink.as_mut() {
            tri!(sink.append(store.conn(), &mut results));
        }
        let c = tri!(queue::commit_retrying(
            store.conn_mut(),
            stage,
            &results,
            COMMIT_PATIENCE,
            || shared.is_cancelled(),
        ));
        timing.commit_secs += t.elapsed().as_secs_f64();
        timing.batches += 1;
        if opts.on_event.is_some() {
            shared.emit(JobEvent::Committed {
                stage,
                file_ids: results.iter().map(|(claim, _)| claim.id).collect(),
            });
        }

        sum.ok += c.ok;
        sum.partial += c.partial;
        sum.errors += c.errors;
        sum.skipped += c.skipped;
        sum.done = sum.ok + sum.partial + sum.errors + sum.skipped;
        sum.seconds = started.elapsed().as_secs_f64();
        sum.timing = timing.clone();
        shared.update(&sum);
        shared.emit(JobEvent::Progress {
            stage,
            done: sum.done,
            total,
            errors: sum.errors,
            skipped: sum.skipped,
        });
    }
    sum.seconds = started.elapsed().as_secs_f64();
    sum.timing = timing;
    shared.emit(JobEvent::StageFinished(sum.clone()));
    Ok(sum)
}
