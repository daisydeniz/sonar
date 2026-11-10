//! The job thread: stages in priority order; each stage's batch loop lives
//! in [`super::batches`].

use std::sync::Arc;
use std::time::Instant;

use rayon::ThreadPoolBuilder;

use super::batches;
use super::progress::{JobEvent, JobSummary, Shared};
use super::stage::Stage;
use super::work::WorkOptions;
use super::JobOptions;
use crate::{Error, Result, Store};

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
        // A panic outside the per-file guard must not abort the process: the
        // dropped result sender makes the coordinator fail with `Internal`.
        .panic_handler(|_| tracing::error!("worker pool task panicked"))
        .build()
        .map_err(|e| Error::Internal {
            reason: format!("cannot start worker pool: {e}"),
        })?;
    let work = Arc::new(WorkOptions {
        delay: opts.debug_file_delay,
        before_file: opts.debug_before_file.clone(),
    });

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
        let s = batches::run_stage(&mut store, stage, opts, &pool, &work, shared)?;
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
