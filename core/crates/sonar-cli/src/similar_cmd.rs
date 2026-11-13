//! `sonar similar`: files that sound like a library file or an external
//! file (ADR-024), with aspect weights and the filters of `sonar search`.

use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Context, Result};
use clap::Args;
use serde::Serialize;
use sonar_core::engine::{Engine, EngineOptions};
use sonar_core::paths;
use sonar_core::similar::{AspectDistances, AspectWeights, Seed, SimilarOptions};
use sonar_core::Store;

use crate::search_args::FilterFlags;

/// `sonar similar` arguments.
#[derive(Debug, Clone, Args)]
pub struct SimilarArgs {
    /// Library file id to start from (`file_id` in `sonar search --json`).
    #[arg(required_unless_present = "file", conflicts_with = "file")]
    pub id: Option<i64>,
    /// An audio file anywhere (inside the library or not), analyzed on
    /// the fly.
    #[arg(long, value_name = "PATH")]
    pub file: Option<PathBuf>,
    /// Aspect weights, e.g. `timbre=2,pitch=1`: listed aspects only
    /// (others 0); `overall=W` adds W to all four. Default: all four 1.
    #[arg(long, value_name = "ASPECT=W,..", value_parser = parse_weights)]
    pub weights: Option<AspectWeights>,
    /// Maximum number of results printed.
    #[arg(long, default_value_t = 20)]
    pub limit: usize,
    /// Print JSON instead of text.
    #[arg(long)]
    pub json: bool,
    /// Which files may be returned (same flags as `sonar search`).
    #[command(flatten)]
    pub filters: FilterFlags,
}

fn parse_weights(s: &str) -> Result<AspectWeights, String> {
    s.parse().map_err(|e: sonar_core::Error| e.to_string())
}

#[derive(Serialize)]
struct SeedInfo {
    file_id: Option<i64>,
    path: Option<PathBuf>,
}

#[derive(Serialize)]
struct Row {
    rank: usize,
    distance: f32,
    file_id: i64,
    path: Option<PathBuf>,
    aspects: AspectDistances,
}

#[derive(Serialize)]
struct Report {
    seed: SeedInfo,
    weights: AspectWeights,
    candidates: usize,
    took_ms: f64,
    hits: Vec<Row>,
}

/// Runs the query against the default store, text index and vector store
/// and prints rank, distance, file id and path.
pub fn run(args: &SimilarArgs) -> Result<()> {
    let store = Store::open_default().context("opening the Sonar store")?;
    // Index sync stays the engine's business; a missing index is built on
    // the first query.
    let engine = Engine::open_with(
        paths::data_dir().context("locating the Sonar store")?,
        EngineOptions {
            rebuild_index: false,
            ..EngineOptions::default()
        },
    )
    .context("opening the Sonar engine")?;
    let (seed, seed_path) = match (&args.file, args.id) {
        (Some(path), _) => (Seed::External(path.clone()), Some(path.clone())),
        (None, Some(id)) => (
            Seed::File(id),
            engine.file_path(id).context("looking up the seed")?,
        ),
        (None, None) => anyhow::bail!("give a file id or --file PATH"),
    };
    let opts = SimilarOptions {
        limit: args.limit,
        filters: args.filters.filters(&store)?,
        weights: args.weights.unwrap_or_default(),
        cancel: None,
    };
    let started = Instant::now();
    let results = engine
        .similar(&seed, &opts)
        .context("similarity search (run `sonar analyze` first if there are no vectors)")?;
    let took_ms = started.elapsed().as_secs_f64() * 1e3;
    let hits = results
        .hits
        .iter()
        .enumerate()
        .map(|(i, h)| {
            Ok(Row {
                rank: i + 1,
                distance: h.distance,
                file_id: h.file_id,
                path: store.file_path(h.file_id)?,
                aspects: h.aspects,
            })
        })
        .collect::<sonar_core::Result<Vec<_>>>()
        .context("looking up result paths")?;
    let report = Report {
        seed: SeedInfo {
            file_id: results.seed_file_id,
            path: seed_path,
        },
        weights: opts.weights,
        candidates: results.candidates,
        took_ms,
        hits,
    };
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_text(&report);
    }
    Ok(())
}

fn print_text(r: &Report) {
    let show = |p: Option<&PathBuf>| {
        p.map_or_else(|| "(not in store)".into(), |p| p.display().to_string())
    };
    let id = r
        .seed
        .file_id
        .map_or_else(String::new, |id| format!("#{id} "));
    println!(
        "similar to {id}{} ({})",
        show(r.seed.path.as_ref()),
        r.weights
    );
    for h in &r.hits {
        println!(
            "{:>4}  {:>7.3}  #{:<7} {}",
            h.rank,
            h.distance,
            h.file_id,
            show(h.path.as_ref())
        );
    }
    println!(
        "{} of {} candidates ({:.1} ms)",
        r.hits.len(),
        r.candidates,
        r.took_ms
    );
}

#[cfg(test)]
mod tests {
    use crate::cli::{Cli, Command};
    use clap::Parser;

    #[test]
    fn parses_seed_weights_and_filters() {
        let cli = Cli::try_parse_from([
            "sonar",
            "similar",
            "12",
            "--weights",
            "pitch=2,timbre=1",
            "--kind",
            "one-shot",
            "--duration",
            "-1",
            "--limit",
            "5",
        ]);
        match cli.map(|c| c.command) {
            Ok(Command::Similar(a)) => {
                assert_eq!(a.id, Some(12));
                assert_eq!(a.limit, 5);
                assert_eq!(a.weights.map(|w| w.as_array()), Some([0.0, 1.0, 2.0, 0.0]));
                assert_eq!(a.filters.kind, ["one-shot"]);
            }
            other => panic!("{other:?}"),
        }
        let cli = Cli::try_parse_from(["sonar", "similar", "--file", "x.wav"]);
        assert!(matches!(cli.map(|c| c.command), Ok(Command::Similar(a)) if a.id.is_none()));
        for bad in [
            &["sonar", "similar"][..],
            &["sonar", "similar", "1", "--file", "x.wav"],
            &["sonar", "similar", "1", "--weights", "color=1"],
            &["sonar", "similar", "1", "--weights", "pitch=0"],
            &["sonar", "similar", "1", "--sort", "bpm"],
        ] {
            assert!(Cli::try_parse_from(bad).is_err(), "{bad:?}");
        }
    }
}
