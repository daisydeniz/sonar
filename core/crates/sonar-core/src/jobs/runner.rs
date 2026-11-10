//! The job thread: stages in priority order, batches claimed from the
//! store, processed on a rayon pool, committed by this thread only (it owns
//! the database connection).

use std::sync::Arc;
use std::time::{Duration, Instant};

use rayon::iter::{IntoParallelIterator, ParallelIterator};
use rayon::ThreadPoolBuilder;

use super::progress::{JobEvent, JobSummary, Shared, StageSummary};
use super::queue::{self, Claim};
use super::stage::{FileResult, Stage};
use super::work::{self, WorkOptions};
use super::JobOptions;
use crate::{Error, Result, Store};

/// How long a batch commit keeps retrying while another writer (e.g. a long
/// scan) holds the database lock, before the job fails.
const COMMIT_PATIENCE: Duration = Duration::from_secs(60);

/// Runs the job to completion (or cancellation).
pub(crate) fn run(mut store: Store, opts: &JobOptions, shared: &Arc<Shared>) -> Result<JobSummary> {
    let started = Instant::now();
    let threads = match opts.threads {
        0 => std::thread::available_parallelism().map_or(1, usize::from),
        n => n,
    };
    let pool = ThreadPoolBuilder::new()
        .num_threads(threads)
        .thread_name(|i| format!("sonar-job-{i}"))
        .build()
        .map_err(|e| Error::Internal {
            reason: format!("cannot start worker pool: {e}"),
        })?;
    let work = WorkOptions {
        delay: opts.debug_file_delay,
        before_file: opts.debug_before_file.clone(),
    };

    // Strict stage order across the store; unimplemented stages are skipped.
    let mut stages: Vec<Stage> = opts
        .stages
        .iter()
        .copied()
        .filter(|s| s.is_runnable())
        .collect();
    stages.sort_unstable();
    stages.dedup();

    let mut summary = JobSummary {
        stages: Vec::new(),
        cancelled: false,
        seconds: 0.0,
    };
    for stage in stages {
        if shared.is_cancelled() {
            break;
        }
        let s = run_stage(&mut store, stage, opts, &pool, &work, shared)?;
        summary.stages.push(s);
    }
    summary.cancelled = shared.is_cancelled();
    summary.seconds = started.elapsed().as_secs_f64();
    shared.emit(if summary.cancelled {
        JobEvent::Cancelled
    } else {
        JobEvent::Finished
    });
    Ok(summary)
}

fn run_stage(
    store: &mut Store,
    stage: Stage,
    opts: &JobOptions,
    pool: &rayon::ThreadPool,
    work: &WorkOptions,
    shared: &Arc<Shared>,
) -> Result<StageSummary> {
    let started = Instant::now();
    let total = queue::count_pending(store.conn(), stage)?;
    let mut sum = StageSummary::new(stage, total);
    shared.begin_stage(stage, total);
    shared.emit(JobEvent::StageStarted { stage, total });

    let batch = opts.batch_size.max(1);
    // Cursor over ids: a skipped file is not offered again in this run.
    let mut after = 0i64;
    while shared.wait_runnable() {
        let claims = queue::claim_batch(store.conn(), stage, after, batch)?;
        let Some(last) = claims.last() else { break };
        after = last.id;

        let results: Vec<(Claim, FileResult)> = pool.install(|| {
            claims
                .into_par_iter()
                .map(|claim| {
                    let result = work::process(stage, &claim, shared, work);
                    (claim, result)
                })
                .collect()
        });
        let c = queue::commit_retrying(store.conn_mut(), stage, &results, COMMIT_PATIENCE, || {
            shared.is_cancelled()
        })?;

        sum.ok += c.ok;
        sum.partial += c.partial;
        sum.errors += c.errors;
        sum.skipped += c.skipped;
        sum.done = sum.ok + sum.partial + sum.errors + sum.skipped;
        sum.seconds = started.elapsed().as_secs_f64();
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
    shared.emit(JobEvent::StageFinished(sum.clone()));
    Ok(sum)
}
