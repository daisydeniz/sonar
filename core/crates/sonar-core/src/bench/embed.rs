//! `sonar bench embed` (T5.2, ADR-027): CLAP embedding throughput.
//!
//! - **model only**: clips/s of the session pool on prepared features
//!   (callers keep the queue full; no decode, no front end, no database);
//! - **front end**: single-thread decode + front end per file on library
//!   files (the CPU work around the model);
//! - **end to end**: the real `Embed` stage over a library of copies of the
//!   fixtures in a throwaway store (decode + front end + model + store +
//!   SQLite commits), files/s. Budget: >= 100 files/s on an M4.

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

use serde::Serialize;

use super::analyze::{bench_analyze, AnalyzeBench, AnalyzeBenchOptions};
use super::{machine, rate, Machine};
use crate::embed::{AudioPrep, EmbedOptions, EmbedPool, FrontEnd};
use crate::jobs::Stage;
use crate::{Error, Result};

/// Options for [`bench_embed`].
#[derive(Debug, Clone)]
pub struct EmbedBenchOptions {
    /// Files in the library of the end-to-end run.
    pub files: usize,
    /// Clips in the model-only run.
    pub model_clips: usize,
    /// Files per committed batch of the end-to-end run.
    pub batch: usize,
    /// Pool settings under test.
    pub embed: EmbedOptions,
}

/// The model-only run.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ModelRun {
    /// Seconds to load every session.
    pub load_secs: f64,
    /// Clips embedded (after a warm-up of as many clips as sessions).
    pub clips: usize,
    /// Wall-clock seconds.
    pub secs: f64,
    /// `clips / secs`.
    pub clips_per_sec: f64,
}

/// Result of [`bench_embed`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EmbedBench {
    /// Where it ran.
    pub machine: Machine,
    /// Execution provider.
    pub ep: String,
    /// Audio sessions.
    pub sessions: usize,
    /// Most clips per session run.
    pub max_batch: usize,
    /// Model only.
    pub model: ModelRun,
    /// Single-thread milliseconds per file for decode + front end.
    pub front_ms_per_file: f64,
    /// Files timed for `front_ms_per_file`.
    pub front_files: usize,
    /// The end-to-end `Embed` stage over the generated library.
    pub end_to_end: AnalyzeBench,
}

impl EmbedBench {
    /// Files per second of the end-to-end stage.
    pub fn files_per_sec(&self) -> f64 {
        self.end_to_end
            .stages
            .first()
            .map_or(0.0, crate::jobs::StageSummary::files_per_sec)
    }
}

fn embed_err(reason: impl Into<String>) -> Error {
    Error::Embed {
        reason: reason.into(),
    }
}

/// Runs the three measurements in `work` (see the module docs).
///
/// # Errors
/// [`Error::Embed`] if the model is not available; the job's errors.
pub fn bench_embed(work: &Path, opts: &EmbedBenchOptions) -> Result<EmbedBench> {
    if let Some(why) = opts.embed.unavailable_reason() {
        return Err(embed_err(why));
    }
    let model = bench_model(opts)?;
    let end_to_end = bench_analyze(
        work,
        &AnalyzeBenchOptions {
            files: opts.files,
            stages: vec![Stage::Embed],
            threads: 0,
            batch: opts.batch,
            warm: true,
            clone: false,
            embed: opts.embed.clone(),
        },
    )?;
    let (front_ms_per_file, front_files) = bench_front(&work.join("library").join("pack000"))?;
    Ok(EmbedBench {
        machine: machine(),
        ep: opts.embed.ep.as_str().to_owned(),
        sessions: opts.embed.resolved_sessions(),
        max_batch: opts.embed.resolved_batch(),
        model,
        front_ms_per_file,
        front_files,
        end_to_end,
    })
}

fn bench_model(opts: &EmbedBenchOptions) -> Result<ModelRun> {
    let t = Instant::now();
    let pool = Arc::new(EmbedPool::new(&opts.embed)?);
    let load_secs = t.elapsed().as_secs_f64();
    let sessions = pool.sessions();
    // Pseudo-noise clip: the cost of the model does not depend on content.
    let mut s = 1u32;
    let wave: Vec<f32> = (0..crate::embed::front::CLIP_SAMPLES)
        .map(|_| {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (s >> 8) as f32 / (1u32 << 23) as f32 - 1.0
        })
        .collect();
    let features = Arc::new(FrontEnd::new().features(&wave)?);
    let callers = (sessions * 2).max(2);
    let run = |total: usize| -> Result<f64> {
        let next = Arc::new(AtomicUsize::new(0));
        let t = Instant::now();
        let handles: Vec<_> = (0..callers)
            .map(|_| {
                let (pool, features, next) =
                    (Arc::clone(&pool), Arc::clone(&features), Arc::clone(&next));
                std::thread::spawn(move || -> Result<()> {
                    while next.fetch_add(1, Ordering::Relaxed) < total {
                        pool.embed(features.as_ref().clone())?;
                    }
                    Ok(())
                })
            })
            .collect();
        for h in handles {
            h.join().map_err(|_| embed_err("bench thread panicked"))??;
        }
        Ok(t.elapsed().as_secs_f64())
    };
    run(sessions * 2)?;
    let clips = opts.model_clips.max(1);
    let secs = run(clips)?;
    Ok(ModelRun {
        load_secs,
        clips,
        secs,
        clips_per_sec: rate(clips as u64, secs),
    })
}

/// Single-thread decode + front end over up to 200 files of `dir`.
fn bench_front(dir: &Path) -> Result<(f64, usize)> {
    let mut prep = AudioPrep::new();
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| Error::io(dir, e))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file())
        .collect();
    files.sort();
    files.truncate(200);
    let t = Instant::now();
    let mut done = 0;
    for f in &files {
        if prep.prepare(f, None).is_ok() {
            done += 1;
        }
    }
    let secs = t.elapsed().as_secs_f64();
    Ok((
        if done > 0 {
            secs * 1000.0 / done as f64
        } else {
            0.0
        },
        done,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_run_reports_all_three_numbers_when_the_model_is_present() {
        let embed = EmbedOptions {
            sessions: 2,
            ..EmbedOptions::default()
        };
        if let Some(why) = embed.unavailable_reason() {
            eprintln!("skipped: {why}");
            return;
        }
        let tmp = tempfile::tempdir().expect("tmp");
        let b = bench_embed(
            tmp.path(),
            &EmbedBenchOptions {
                files: 40,
                model_clips: 20,
                batch: 16,
                embed,
            },
        )
        .expect("bench");
        assert_eq!((b.sessions, b.model.clips), (2, 20));
        assert!(b.model.clips_per_sec > 0.0 && b.files_per_sec() > 0.0);
        let st = &b.end_to_end.stages[0];
        // The fixtures contain one deliberately zero-length file
        // (`edge_002`), which embedding rejects by design (ADR-026); it falls
        // in the first 40 files of the library, deterministically.
        assert_eq!(
            (st.stage, st.total, st.done, st.errors),
            (Stage::Embed, 40, 40, 1)
        );
        assert!(b.front_files > 0);
    }
}
