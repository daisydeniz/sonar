//! `sonar bench scan|decode|analyze`: performance benchmarks (use a
//! release build: `cargo run --release -p sonar-cli -- bench ...`).
//!
//! Benchmarks run in a temporary working folder with their own store, never
//! the user's.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Subcommand;
use serde::Serialize;
use sonar_core::bench::{
    self, AnalyzeBench, AnalyzeBenchOptions, DecodeBench, DecodeBenchOptions, PipelineRun,
    ScanBench, ScanPhase, WatchBench, WatchBenchOptions, WatchPhase,
};
use sonar_core::jobs::Stage;

/// `sonar bench` subcommands.
#[derive(Debug, Subcommand)]
pub enum BenchCommand {
    /// Generate a library of many small files and time the first scan, a
    /// no-op rescan and a rescan after ~1 % edits (budget: > 2,000 files/s).
    Scan {
        /// Number of audio files to generate.
        #[arg(long, default_value_t = 50_000, value_parser = clap::value_parser!(u32).range(1..=5_000_000))]
        files: u32,
        /// Create the working folder inside this directory (default: the
        /// system temp dir), e.g. to benchmark an external drive.
        #[arg(long)]
        dir: Option<PathBuf>,
        /// Keep the working folder (library and store) and print its path.
        #[arg(long)]
        keep: bool,
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },

    /// Decode throughput (files/s, x realtime) over a folder of audio
    /// files, with the peak/loudness pass off and on.
    Decode {
        /// Folder to decode (e.g. `sonar fixtures` output). Default:
        /// generate the fixtures into a temporary folder.
        #[arg(long)]
        dir: Option<PathBuf>,
        /// Worker threads (0 = all cores).
        #[arg(long, default_value_t = 1)]
        threads: usize,
        /// Walks over the file set per timed run.
        #[arg(long, default_value_t = 3, value_parser = clap::value_parser!(u32).range(1..))]
        repeat: u32,
        /// Without --dir: leave out the > 5 min fixture.
        #[arg(long)]
        skip_long: bool,
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },

    /// Run the job pipeline over a generated library (copies of the
    /// fixtures) in a temporary store and report files/s per stage.
    Analyze {
        /// Files in the library.
        #[arg(long, default_value_t = 3_000, value_parser = clap::value_parser!(u32).range(1..=1_000_000))]
        files: u32,
        /// Stages to run, comma separated (`meta`, `dsp`).
        #[arg(long, value_delimiter = ',', default_values = ["meta", "dsp"])]
        stages: Vec<Stage>,
        /// Worker threads (0 = all cores).
        #[arg(long, default_value_t = 0)]
        threads: usize,
        /// Files per committed batch.
        #[arg(long, default_value_t = 128, value_parser = clap::value_parser!(u32).range(1..))]
        batch: u32,
        /// Create the working folder inside this directory.
        #[arg(long)]
        dir: Option<PathBuf>,
        /// Keep the working folder and print its path. (The folder path is
        /// always printed to stderr at start; after Ctrl-C delete it by hand.)
        #[arg(long)]
        keep: bool,
        /// Skip the untimed cache warm-up pass. For a truly cold cache run
        /// `sudo purge` first; files that were just written may still be cached.
        #[arg(long)]
        cold: bool,
        /// Make the library files APFS clones of the fixtures (`fs::copy`)
        /// instead of writing them. New clones have no cached pages, so with
        /// `--cold` the stages read from disk without `sudo purge`.
        #[arg(long)]
        clone: bool,
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },

    /// Watcher under a continuous copy into a big library (scan count and
    /// duration, duty cycle, catch-up latency), optionally while the job
    /// pipeline analyzes the same store (SQLite contention).
    Watch {
        /// Files in the base library.
        #[arg(long, default_value_t = 50_000, value_parser = clap::value_parser!(u32).range(1..=5_000_000))]
        files: u32,
        /// New files per copy phase.
        #[arg(long, default_value_t = 2_000, value_parser = clap::value_parser!(u32).range(1..=1_000_000))]
        copy_files: u32,
        /// Copy rate in files/s (default: as fast as possible).
        #[arg(long)]
        copy_rate: Option<f64>,
        /// Also measure the pipeline alone and during a second copy.
        #[arg(long)]
        with_analyze: bool,
        /// Pipeline stages for --with-analyze, comma separated.
        #[arg(long, value_delimiter = ',', default_values = ["meta", "dsp"])]
        stages: Vec<Stage>,
        /// Create the working folder inside this directory.
        #[arg(long)]
        dir: Option<PathBuf>,
        /// Keep the working folder and print its path.
        #[arg(long)]
        keep: bool,
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
}

/// Runs a benchmark and prints its result to stdout.
pub fn run(cmd: BenchCommand) -> Result<()> {
    match cmd {
        BenchCommand::Scan {
            files,
            dir,
            keep,
            json,
        } => {
            let work = Work::new(dir.as_deref(), keep)?;
            let b = bench::bench_scan(work.path(), files as usize).context("scan benchmark")?;
            emit(json, &b, print_scan)
        }
        BenchCommand::Decode {
            dir,
            threads,
            repeat,
            skip_long,
            json,
        } => {
            let opts = DecodeBenchOptions {
                threads,
                repeat: repeat as usize,
            };
            let b = match dir {
                Some(dir) => bench::bench_decode(&dir, opts),
                None => {
                    let work = Work::new(None, false)?;
                    bench::decode::generate_fixtures(work.path(), !skip_long)
                        .context("generating fixtures")?;
                    bench::bench_decode(work.path(), opts)
                }
            }
            .context("decode benchmark")?;
            emit(json, &b, print_decode)
        }
        BenchCommand::Analyze {
            files,
            stages,
            threads,
            batch,
            dir,
            keep,
            cold,
            clone,
            json,
        } => {
            let work = Work::new(dir.as_deref(), keep)?;
            let opts = AnalyzeBenchOptions {
                files: files as usize,
                stages,
                threads,
                batch: batch as usize,
                warm: !cold,
                clone,
            };
            let b = bench::bench_analyze(work.path(), &opts).context("analyze benchmark")?;
            emit(json, &b, print_analyze)
        }
        BenchCommand::Watch {
            files,
            copy_files,
            copy_rate,
            with_analyze,
            stages,
            dir,
            keep,
            json,
        } => {
            if copy_rate.is_some_and(|r| !r.is_finite() || r <= 0.0) {
                anyhow::bail!("--copy-rate must be a positive number of files per second");
            }
            let work = Work::new(dir.as_deref(), keep)?;
            let opts = WatchBenchOptions {
                files: files as usize,
                copy_files: copy_files as usize,
                copy_rate,
                with_analyze,
                stages,
                ..WatchBenchOptions::default()
            };
            let b = bench::bench_watch(work.path(), &opts).context("watch benchmark")?;
            emit(json, &b, print_watch)
        }
    }
}

fn emit<T: Serialize>(json: bool, value: &T, text: fn(&T)) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(value)?);
    } else {
        text(value);
    }
    Ok(())
}

/// A working folder that is deleted on drop unless kept.
struct Work {
    path: PathBuf,
    _guard: Option<tempfile::TempDir>,
}

impl Work {
    fn new(parent: Option<&Path>, keep: bool) -> Result<Self> {
        let mut b = tempfile::Builder::new();
        b.prefix("sonar-bench-");
        let tmp = match parent {
            Some(p) => b.tempdir_in(p),
            None => b.tempdir(),
        }
        .context("creating the working folder")?;
        if keep {
            let path = tmp.keep();
            eprintln!("working folder (kept): {}", path.display());
            Ok(Self { path, _guard: None })
        } else {
            eprintln!(
                "working folder: {} (deleted at the end; remove by hand if interrupted)",
                tmp.path().display()
            );
            Ok(Self {
                path: tmp.path().to_path_buf(),
                _guard: Some(tmp),
            })
        }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

fn machine_line(m: &bench::Machine) -> String {
    format!("{}, {} cores, {} build", m.cpu, m.cores, m.profile)
}

fn print_scan(b: &ScanBench) {
    println!("machine: {}", machine_line(&b.machine));
    println!(
        "corpus: {} files in {} folders, {:.1} MB (generated in {:.1} s, untimed)",
        b.files,
        b.dirs,
        b.bytes as f64 / 1e6,
        b.generate_secs
    );
    let phase = |name: &str, p: &ScanPhase| {
        println!(
            "{name}: {:.3} s, {:.0} files/s (added {}, changed {}, moved {}, removed {}, restored {}, unchanged {}, errors {})",
            p.secs,
            p.files_per_sec,
            p.added,
            p.changed,
            p.moved,
            p.removed,
            p.restored,
            p.unchanged,
            p.errors
        );
    };
    phase("first scan   ", &b.first);
    phase("no-op rescan ", &b.noop);
    println!(
        "edits: {} changed, {} renamed, {} deleted, {} added",
        b.edits.changed, b.edits.renamed, b.edits.deleted, b.edits.added
    );
    phase("edited rescan", &b.incremental);
    println!("database: {:.1} MB", b.db_bytes as f64 / 1e6);
}

fn print_decode(b: &DecodeBench) {
    println!("machine: {}", machine_line(&b.machine));
    println!(
        "files: {} found, {} decodable; {} thread(s), {} pass(es) per run",
        b.files_found, b.files_ok, b.threads, b.repeat
    );
    for r in &b.runs {
        println!(
            "meta {:<3}: {:.3} s, {:.0} files/s, {:.0} x realtime ({:.0} s of source audio)",
            if r.meta { "on" } else { "off" },
            r.secs,
            r.files_per_sec,
            r.realtime_x,
            r.source_audio_secs
        );
    }
}

fn print_watch_phase(p: &WatchPhase) {
    println!(
        "  copy: {} files, {:.1} MB in {:.2} s",
        p.copy_files,
        p.bytes_written as f64 / 1e6,
        p.copy_secs
    );
    println!(
        "  watcher scans: {} (min {} / median {} / max {} ms), duty cycle over the copy {:.0} %",
        p.scans.count,
        p.scans.min_ms,
        p.scans.median_ms,
        p.scans.max_ms,
        p.duty_cycle * 100.0
    );
    println!(
        "  catch-up after copy end: {:.2} s ({}); index {} of {} files ({})",
        p.catch_up_secs,
        if p.caught_up {
            "caught up"
        } else {
            "TIMED OUT"
        },
        p.db_files,
        p.expected_files,
        if p.verified { "verified" } else { "MISMATCH" }
    );
    println!("  watcher errors: {}", p.watcher_errors);
    for m in &p.error_messages {
        println!("    {m}");
    }
}

fn print_pipeline(name: &str, r: &PipelineRun) {
    println!(
        "  {name}: {} run(s), {} files in {:.2} s active, {:.0} files/s, longest gap between progress events {} ms",
        r.runs, r.files_done, r.active_secs, r.files_per_sec, r.max_progress_gap_ms
    );
    if let Some(e) = &r.error {
        println!("    pipeline FAILED: {e}");
    }
}

fn print_watch(b: &WatchBench) {
    println!("machine: {}", machine_line(&b.machine));
    println!(
        "library: {} files (generated in {:.1} s, scanned in {:.2} s, untimed); watcher start-up scan {} ms; copy rate {}",
        b.files,
        b.generate_secs,
        b.initial_scan_secs,
        b.watcher_start_scan_ms,
        b.copy_rate
            .map_or("unthrottled".to_owned(), |r| format!("{r:.0} files/s"))
    );
    println!("copy only:");
    print_watch_phase(&b.copy_only);
    if let Some(base) = &b.baseline {
        println!("pipeline alone:");
        print_pipeline("no contention", base);
    }
    if let Some(c) = &b.contended {
        println!(
            "copy + watcher + pipeline ({} scans overlapped a pipeline run, {} ms in total):",
            c.overlapping_scans, c.overlapping_scan_ms
        );
        print_watch_phase(&c.watch);
        print_pipeline("with contention", &c.pipeline);
    }
}

fn print_analyze(b: &AnalyzeBench) {
    println!("machine: {}", machine_line(&b.machine));
    println!(
        "library: {} files ({:.1} MB, copies of {} fixtures); threads {}, batch {}",
        b.files,
        b.bytes as f64 / 1e6,
        b.distinct_sources,
        b.threads,
        b.batch
    );
    println!(
        "setup (untimed): write {:.1} s, scan {:.1} s, cache warm-up {:.1} s",
        b.generate_secs, b.scan_secs, b.warmup_secs
    );
    println!(
        "page cache: {}",
        match (b.warm_cache, b.cloned) {
            (true, _) =>
                "warm (every file read once before the stages; numbers are CPU + database work)",
            (false, true) => "cold (--cold --clone: fresh APFS clones, no cached pages)",
            (false, false) =>
                "as left by the writes (--cold; add --clone or run `sudo purge` for a truly cold cache)",
        }
    );
    for s in &b.stages {
        println!(
            "{}: {} files, {:.3} s, {:.0} files/s (ok {}, partial {}, errors {}, skipped {})",
            s.stage,
            s.done,
            s.seconds,
            s.files_per_sec(),
            s.ok,
            s.partial,
            s.errors,
            s.skipped
        );
    }
    println!("job total: {:.3} s", b.job_secs);
}
