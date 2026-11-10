//! `sonar index` (rebuild the text index) and `sonar search` (query it),
//! plus the refresh `sonar scan` runs after scanning.

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result};
use serde::Serialize;
use sonar_core::index::{OpenState, TextIndex};
use sonar_core::search::{search, SearchOptions, Synonyms};
use sonar_core::Store;

/// `sonar search` arguments.
pub struct Args<'a> {
    /// Query words (joined with spaces).
    pub query: &'a [String],
    /// Maximum hits printed.
    pub limit: usize,
    /// Print JSON.
    pub json: bool,
    /// Disable as-you-type prefix matching of the last word.
    pub no_prefix: bool,
    /// Synonyms file instead of `<SONAR_HOME>/synonyms.tsv` / built-in.
    pub synonyms: Option<&'a Path>,
}

#[derive(Serialize)]
struct Row {
    rank: usize,
    score: f32,
    file_id: i64,
    path: Option<PathBuf>,
}

#[derive(Serialize)]
struct Report<'a> {
    query: &'a str,
    total: usize,
    took_ms: f64,
    hits: Vec<Row>,
}

/// `sonar index`: rebuild the text index from the store.
pub fn index(json: bool) -> Result<()> {
    let store = Store::open_default().context("opening the Sonar store")?;
    let (ix, _) = TextIndex::open_default().context("opening the search index")?;
    let report = ix.rebuild(&store).context("building the search index")?;
    let dir = ix.dir().map(Path::to_path_buf);
    let secs = report.elapsed.as_secs_f64();
    if json {
        let v = serde_json::json!({ "docs": report.docs, "seconds": secs, "dir": dir });
        println!("{}", serde_json::to_string_pretty(&v)?);
    } else {
        let rate = report.docs as f64 / secs.max(1e-9);
        println!(
            "indexed {} files in {:.3} s ({:.0} files/s)",
            report.docs, secs, rate
        );
        if let Some(dir) = dir {
            println!("index: {}", dir.display());
        }
    }
    Ok(())
}

/// `sonar search`: query the index (building it first if it is missing or
/// from another version) and print rank, score, file id and path.
pub fn run(args: &Args<'_>) -> Result<()> {
    let store = Store::open_default().context("opening the Sonar store")?;
    let (ix, state) = TextIndex::open_default().context("opening the search index")?;
    if let OpenState::NeedsRebuild(reason) = state {
        eprintln!("building the search index ({reason})...");
        let r = ix.rebuild(&store).context("building the search index")?;
        eprintln!(
            "indexed {} files in {:.2} s",
            r.docs,
            r.elapsed.as_secs_f64()
        );
    }
    let synonyms = match args.synonyms {
        Some(path) => {
            Synonyms::load(path).with_context(|| format!("reading synonyms {}", path.display()))?
        }
        None => Synonyms::load_default().context("reading synonyms")?,
    };
    let query = args.query.join(" ");
    let opts = SearchOptions {
        limit: args.limit,
        prefix: !args.no_prefix,
    };
    let started = Instant::now();
    let results = search(&ix, &synonyms, &query, &opts).context("searching")?;
    let took_ms = started.elapsed().as_secs_f64() * 1e3;
    let hits = results
        .hits
        .iter()
        .enumerate()
        .map(|(i, h)| {
            Ok(Row {
                rank: i + 1,
                score: h.score,
                file_id: h.file_id,
                path: store.file_path(h.file_id)?,
            })
        })
        .collect::<sonar_core::Result<Vec<_>>>()
        .context("looking up result paths")?;
    let report = Report {
        query: &query,
        total: results.total,
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

fn print_text(r: &Report<'_>) {
    for h in &r.hits {
        let path = h
            .path
            .as_deref()
            .map_or_else(|| "(not in store)".into(), |p| p.display().to_string());
        println!("{:>4}  {:>7.3}  #{:<7} {path}", h.rank, h.score, h.file_id);
    }
    println!(
        "{} of {} results ({:.1} ms)",
        r.hits.len(),
        r.total,
        r.took_ms
    );
}

/// After `sonar scan`: rebuild the search index if one exists, so the next
/// `sonar search` sees renames and new files. Failures only warn (the scan
/// itself succeeded); a missing index is built by `sonar search` later.
pub fn refresh_after_scan(store: &Store) {
    let Ok(dir) = TextIndex::default_dir() else {
        return;
    };
    if !dir.exists() {
        return;
    }
    let result = TextIndex::open(&dir).and_then(|(ix, _)| ix.rebuild(store));
    match result {
        Ok(r) => println!(
            "search index: {} files ({:.2} s)",
            r.docs,
            r.elapsed.as_secs_f64()
        ),
        Err(e) => eprintln!("warning: search index not updated: {e} (run `sonar index`)"),
    }
}
