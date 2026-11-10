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
    self, AnalyzeBench, AnalyzeBenchOptions, DecodeBench, DecodeBenchOptions, ScanBench, ScanPhase,
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
            json,
        } => {
            let work = Work::new(dir.as_deref(), keep)?;
            let opts = AnalyzeBenchOptions {
                files: files as usize,
                stages,
                threads,
                batch: batch as usize,
                warm: !cold,
            };
            let b = bench::bench_analyze(work.path(), &opts).context("analyze benchmark")?;
            emit(json, &b, print_analyze)
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
        if b.warm_cache {
            "warm (every file read once before the stages; numbers are CPU + database work)"
        } else {
            "as left by the writes (--cold; run `sudo purge` first for a truly cold cache)"
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
