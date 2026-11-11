//! `sonar` — headless entry point to every core feature.

mod analyze_cmd;
mod aspects_cmd;
mod bench_cmd;
mod bench_print;
mod bpm_cmd;
mod classify_cmd;
mod cli;
mod decode_cmd;
mod dsp_cmd;
mod eval_cmd;
mod key_cmd;
mod meta_cmd;
mod search_args;
mod search_cmd;
mod stats_cmd;
mod tokenize_cmd;
mod vectors_cmd;
mod watch_cmd;

use anyhow::{Context, Result};
use clap::Parser;
use sonar_core::fixtures::{self, FixtureOptions};
use sonar_core::{scan, Store};
use std::io::IsTerminal;
use std::path::Path;
use tracing_subscriber::EnvFilter;

use crate::cli::{Cli, Command, StoreCommand};

/// Filter used when `RUST_LOG` is unset or invalid.
const DEFAULT_LOG_FILTER: &str = "warn";

fn main() -> Result<()> {
    init_logging()?;
    let cli = Cli::parse();
    tracing::debug!(?cli, "parsed arguments");

    match cli.command {
        Command::Version => println!("sonar {}", sonar_core::version()),
        Command::Fixtures {
            dir,
            seed,
            skip_long,
        } => fixtures(&dir, seed, !skip_long)?,
        Command::Scan { dir } => scan(dir.as_deref())?,
        Command::Decode {
            file,
            rate,
            max_secs,
            full,
            json,
        } => decode_cmd::run(&file, rate, (!full).then_some(max_secs), json)?,
        Command::Meta {
            files,
            max_meta_secs,
            json,
        } => meta_cmd::run(&files, max_meta_secs, json)?,
        Command::Dsp { files, json } => dsp_cmd::run(&files, json)?,
        Command::Aspects { files, json } => aspects_cmd::run(&files, json)?,
        Command::Classify {
            files,
            json,
            no_names,
        } => classify_cmd::run(&files, json, !no_names)?,
        Command::Bpm {
            files,
            json,
            no_names,
        } => bpm_cmd::run(&files, json, !no_names)?,
        Command::Key(args) => key_cmd::run(&args)?,
        Command::Eval(args) => eval_cmd::run(&args)?,
        Command::Analyze {
            stages,
            threads,
            batch,
            json,
            debug_delay_ms,
        } => analyze_cmd::run(analyze_cmd::Args {
            stages,
            threads,
            batch: batch as usize,
            json,
            debug_delay_ms,
        })?,
        Command::Stats { json } => stats_cmd::run(json)?,
        Command::Vectors { compact, json } => vectors_cmd::run(compact, json)?,
        Command::Tokenize {
            inputs,
            query,
            json,
        } => tokenize_cmd::run(&inputs, query, json)?,
        Command::Index { json } => search_cmd::index(json)?,
        Command::Search {
            query,
            limit,
            json,
            no_prefix,
            synonyms,
            filters,
        } => search_cmd::run(&search_cmd::Args {
            query: &query,
            limit,
            json,
            no_prefix,
            synonyms: synonyms.as_deref(),
            filters: &filters,
        })?,
        Command::Bench { command } => bench_cmd::run(command)?,
        Command::Watch { json, debounce_ms } => watch_cmd::run(json, debounce_ms)?,
        Command::Store {
            command: StoreCommand::Info,
        } => store_info()?,
    }
    Ok(())
}

/// `sonar scan [DIR]`: add DIR as a library if needed and scan it, or
/// rescan every library. Prints one report per library.
fn scan(dir: Option<&Path>) -> Result<()> {
    let mut store = Store::open_default().context("opening the Sonar store")?;
    let ids = match dir {
        Some(dir) => match store.add_library(dir) {
            Ok(lib) => vec![lib.id],
            // A registered root that is gone (e.g. unmounted drive) is
            // scanned anyway, so the user gets its `[offline]` report.
            Err(add_err) => match store.library_by_root(dir).context("looking up library")? {
                Some(lib) => vec![lib.id],
                None => {
                    return Err(add_err)
                        .with_context(|| format!("adding library {}", dir.display()))
                }
            },
        },
        None => {
            let libs = store.libraries().context("listing libraries")?;
            if libs.is_empty() {
                println!("no libraries yet: run `sonar scan <dir>`");
            }
            libs.into_iter().map(|l| l.id).collect()
        }
    };
    for id in ids {
        let report =
            scan::scan_library(&mut store, id).with_context(|| format!("scanning library {id}"))?;
        print_report(&report);
    }
    search_cmd::refresh_after_scan(&store);
    Ok(())
}

fn print_report(r: &scan::ScanReport) {
    let state = if r.offline { " [offline]" } else { "" };
    println!(
        "library {}: {}{state} ({:.2} s)",
        r.library_id,
        r.root.display(),
        r.duration.as_secs_f64()
    );
    let c = &r.counts;
    println!(
        "  added {}, changed {}, moved {}, removed {}, restored {}, unchanged {}, errors {}",
        c.added, c.changed, c.moved, c.removed, c.restored, c.unchanged, c.errors
    );
    if !r.issues.is_empty() {
        println!("  issues ({}):", r.issues.len());
        for issue in &r.issues {
            println!("    {issue}");
        }
    }
}

/// `sonar store info`: open the default store and print a summary.
fn store_info() -> Result<()> {
    let store = Store::open_default().context("opening the Sonar store")?;
    let info = store.info().context("reading store info")?;
    let path = info
        .path
        .as_deref()
        .map_or_else(|| "(in memory)".into(), |p| p.display().to_string());
    println!("path: {path}");
    println!("schema version: {}", info.schema_version);
    println!("journal mode: {}", info.journal_mode);
    println!("tables:");
    let width = info.tables.iter().map(|t| t.name.len()).max().unwrap_or(0);
    for t in &info.tables {
        println!("  {:<width$}  {}", t.name, t.rows);
    }
    Ok(())
}

/// `sonar fixtures`: generate, then print a short summary.
fn fixtures(dir: &Path, seed: u64, include_long: bool) -> Result<()> {
    let opts = FixtureOptions { seed, include_long };
    let manifest = fixtures::generate(dir, &opts)
        .with_context(|| format!("generating fixtures in {}", dir.display()))?;
    let bytes = manifest.total_bytes();
    println!(
        "wrote {} fixtures ({:.1} MB, seed {seed}) to {}",
        manifest.fixtures.len(),
        bytes as f64 / 1e6,
        dir.display()
    );
    println!("manifest: {}", dir.join(fixtures::MANIFEST_FILE).display());
    Ok(())
}

/// Logs to stderr (stdout stays clean for command output), filtered by
/// `RUST_LOG`, defaulting to [`DEFAULT_LOG_FILTER`].
fn init_logging() -> Result<()> {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(DEFAULT_LOG_FILTER));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(std::io::stderr().is_terminal())
        .try_init()
        .map_err(|e| anyhow::anyhow!("failed to initialise logging: {e}"))
}
