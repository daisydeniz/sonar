//! The job thread: stages in priority order; each stage's batch loop lives
//! in [`super::batches`].

use std::sync::Arc;
use std::time::Instant;

use rayon::ThreadPoolBuilder;

use super::batches::Sink;
use super::progress::{JobEvent, JobSummary, Shared, SkippedStage, StageSummary};
use super::stage::Stage;
use super::work::WorkOptions;
use super::JobOptions;
use super::{batches, embed, vectors};
use crate::embed::EmbedPool;
use crate::{Error, Result, Store};

fn build_pool(threads: usize) -> Result<rayon::ThreadPool> {
    ThreadPoolBuilder::new()
        .num_threads(threads)
        .thread_name(|i| format!("sonar-job-{i}"))
        // A panic outside the per-file guard must not abort the process: the
        // dropped result sender makes the coordinator fail with `Internal`.
        .panic_handler(|_| tracing::error!("worker pool task panicked"))
        .build()
        .map_err(|e| Error::Internal {
            reason: format!("cannot start worker pool: {e}"),
        })
}

/// Runs the job to completion (or cancellation).
pub(crate) fn run(mut store: Store, opts: &JobOptions, shared: &Arc<Shared>) -> Result<JobSummary> {
    let started = Instant::now();
    let threads = match opts.threads {
        0 => std::thread::available_parallelism().map_or(1, usize::from),
        n => n,
    };
    let pool = build_pool(threads)?;
    let work = Arc::new(WorkOptions {
        delay: opts.debug_file_delay,
        before_file: opts.debug_before_file.clone(),
        embedder: None,
    });

    // Strict stage order across the store.
    let mut stages: Vec<Stage> = opts.stages.clone();
    stages.sort_unstable();
    stages.dedup();

    let mut summary = JobSummary {
        stages: Vec::new(),
        skipped: Vec::new(),
        cancelled: false,
        seconds: 0.0,
    };
    for stage in stages {
        if shared.is_cancelled() {
            break;
        }
        let s = match stage {
            Stage::Dsp => {
                let mut vs = vectors::open(&mut store)?;
                let s = batches::run_stage(
                    &mut store,
                    stage,
                    opts,
                    &pool,
                    &work,
                    shared,
                    Some(Sink::Aspects(&mut vs)),
                )?;
                if !shared.is_cancelled() {
                    vectors::finish(&mut vs)?;
                }
                s
            }
            Stage::Embed => match run_embed(&mut store, opts, &work, shared)? {
                Ok(s) => s,
                Err(reason) => {
                    tracing::info!(%reason, "embed stage skipped");
                    shared.emit(JobEvent::StageSkipped {
                        stage,
                        reason: reason.clone(),
                    });
                    summary.skipped.push(SkippedStage { stage, reason });
                    continue;
                }
            },
            Stage::Meta => batches::run_stage(&mut store, stage, opts, &pool, &work, shared, None)?,
        };
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

/// The `embed` stage: lowest priority, on the session pool with its own
/// rayon pool for decode + front end (enough workers to keep every session
/// fed). `Ok(Err(reason))` = the stage cannot run (no model, no CLAP
/// support, model fails to load): skipped, nothing is recorded per file.
fn run_embed(
    store: &mut Store,
    opts: &JobOptions,
    work: &Arc<WorkOptions>,
    shared: &Arc<Shared>,
) -> Result<std::result::Result<StageSummary, String>> {
    if let Some(reason) = opts.embed.unavailable_reason() {
        return Ok(Err(reason));
    }
    let sessions = opts.embed.resolved_sessions();
    let model = match EmbedPool::new(&opts.embed) {
        Ok(m) => Arc::new(m),
        Err(e) => return Ok(Err(e.to_string())),
    };
    let rayon = build_pool(sessions + sessions / 2 + 2)?;
    let work = Arc::new(WorkOptions {
        embedder: Some(model),
        ..(**work).clone()
    });
    let mut es = embed::open(store)?;
    let s = batches::run_stage(
        store,
        Stage::Embed,
        opts,
        &rayon,
        &work,
        shared,
        Some(Sink::Embeddings(&mut es)),
    )?;
    Ok(Ok(s))
}
