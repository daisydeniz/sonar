//! `sonar bench dupes`: duplicate finder timing on a synthetic library
//! (ADR-036; target: near pass <= 10 s at 500k files).

use anyhow::{Context, Result};
use clap::Args;
use sonar_core::bench::{self, DupesBench, DupesBenchOptions};
use sonar_core::dupes::DEFAULT_NEAR_THRESHOLD;

use crate::bench_cmd::Work;

/// Near pass budget at 500k files (ms).
const NEAR_BUDGET_MS: f64 = 10_000.0;

/// `sonar bench dupes` arguments.
#[derive(Debug, Clone, Args)]
pub struct DupesBenchArgs {
    /// Files in the synthetic library.
    #[arg(long, default_value_t = 500_000, value_parser = clap::value_parser!(u32).range(1..=5_000_000))]
    docs: u32,
    /// Timed runs.
    #[arg(long, default_value_t = 3, value_parser = clap::value_parser!(u32).range(1..=100))]
    runs: u32,
    /// Threads (0 = all cores, as in the app).
    #[arg(long, default_value_t = 0)]
    threads: usize,
    /// Near threshold.
    #[arg(long, default_value_t = DEFAULT_NEAR_THRESHOLD)]
    threshold: f32,
    /// Corpus seed (the default makes runs comparable).
    #[arg(long, default_value_t = bench::search::DEFAULT_SEED)]
    seed: u64,
    /// Create the working folder inside this directory.
    #[arg(long)]
    dir: Option<std::path::PathBuf>,
    /// Keep the working folder and print its path.
    #[arg(long)]
    keep: bool,
    /// Print JSON instead of text.
    #[arg(long)]
    json: bool,
}

/// Runs the benchmark; fails if a run's near pass is over budget at
/// >= 500k files.
pub fn run(a: &DupesBenchArgs) -> Result<()> {
    let work = Work::new(a.dir.as_deref(), a.keep)?;
    let opts = DupesBenchOptions {
        docs: a.docs as usize,
        runs: a.runs as usize,
        seed: a.seed,
        threads: a.threads,
        threshold: a.threshold,
    };
    let b = bench::bench_dupes(work.path(), &opts).context("dupes benchmark")?;
    if a.json {
        println!("{}", serde_json::to_string_pretty(&b)?);
    } else {
        print(&b);
    }
    let worst = b.runs.iter().map(|r| r.near_ms).fold(0.0, f64::max);
    if b.docs >= 500_000 && worst > NEAR_BUDGET_MS {
        anyhow::bail!("near pass {worst:.0} ms over the {NEAR_BUDGET_MS:.0} ms budget");
    }
    Ok(())
}

fn print(b: &DupesBench) {
    println!(
        "machine: {} ({} cores, {}); pool {} threads",
        b.machine.cpu, b.machine.cores, b.machine.profile, b.threads
    );
    println!(
        "corpus: {} files, {} vector rows; planted {} exact candidates, {} near pairs; setup {:.1} s (untimed)",
        b.docs, b.vector_rows, b.planted_exact, b.planted_near, b.setup_secs
    );
    println!(
        "last run: {} files, {} exact candidates (not on disk: skipped), {} near files, \
         {} pairs in the duration window, {} near groups, planted pairs found {}/{}",
        b.files,
        b.exact_candidates,
        b.near_files,
        b.near_pairs,
        b.near_groups,
        b.planted_near_found,
        b.planted_near
    );
    for (i, r) in b.runs.iter().enumerate() {
        println!(
            "run {}: load {:.0} ms, exact {:.0} ms, near {:.0} ms, total {:.0} ms",
            i + 1,
            r.load_ms,
            r.exact_ms,
            r.near_ms,
            r.load_ms + r.exact_ms + r.near_ms
        );
    }
}
