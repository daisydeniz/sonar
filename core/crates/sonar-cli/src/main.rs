//! `sonar` — headless entry point to every core feature.

mod cli;

use anyhow::{Context, Result};
use clap::Parser;
use sonar_core::fixtures::{self, FixtureOptions};
use std::io::IsTerminal;
use std::path::Path;
use tracing_subscriber::EnvFilter;

use crate::cli::{Cli, Command};

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
