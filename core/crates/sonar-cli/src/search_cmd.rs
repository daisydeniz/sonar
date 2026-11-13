//! `sonar index` (rebuild the text index) and `sonar search` (query it).

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result};
use serde::Serialize;
use sonar_core::engine::{Engine, EngineOptions, SearchRequest};
use sonar_core::index::TextIndex;
use sonar_core::paths;
use sonar_core::search::{FacetCount, Facets, SearchMode};
use sonar_core::Store;

use crate::search_args::FilterArgs;

/// `sonar search` arguments.
pub struct Args<'a> {
    /// Filters, sort, facets.
    pub filters: &'a FilterArgs,
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
    /// Search mode; `None` = hybrid when CLAP is available, else text.
    pub mode: Option<SearchMode>,
}

#[derive(Serialize)]
struct Row {
    rank: usize,
    score: f32,
    file_id: i64,
    text_rank: Option<u32>,
    semantic_rank: Option<u32>,
    path: Option<PathBuf>,
}

#[derive(Serialize)]
struct Report<'a> {
    query: &'a str,
    mode: SearchMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    fallback: Option<String>,
    /// Files the text leg matched.
    total: usize,
    /// Filtered files with a CLAP embedding (semantic / hybrid).
    semantic_candidates: usize,
    took_ms: f64,
    hits: Vec<Row>,
    #[serde(skip_serializing_if = "Option::is_none")]
    facets: Option<Facets>,
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
/// from another version) and print rank, score, file id and path. Runs
/// through the [`Engine`] (hybrid fusion, text-only fallback).
pub fn run(args: &Args<'_>) -> Result<()> {
    let store = Store::open_default().context("opening the Sonar store")?;
    let home = paths::data_dir().context("locating the Sonar store")?;
    let engine = Engine::open_with(
        home,
        EngineOptions {
            synonyms_file: args.synonyms.map(Path::to_path_buf),
            ..EngineOptions::default()
        },
    )
    .context("opening the Sonar engine")?;
    if let Some((reason, r)) = engine.startup_rebuild() {
        eprintln!("building the search index ({reason})...");
        eprintln!(
            "indexed {} files in {:.2} s",
            r.docs,
            r.elapsed.as_secs_f64()
        );
    }
    let query = args.query.join(" ");
    let mode = args.mode.unwrap_or_default();
    let request = SearchRequest {
        query: query.clone(),
        mode,
        filters: args.filters.flags.filters(&store)?,
        sort: args.filters.sort.unwrap_or_default(),
        offset: 0,
        limit: args.limit,
        prefix: !args.no_prefix,
        facets: args.filters.facets,
    };
    let started = Instant::now();
    let results = engine.search(&request).context("searching")?;
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
                text_rank: h.text_rank,
                semantic_rank: h.semantic_rank,
                path: store.file_path(h.file_id)?,
            })
        })
        .collect::<sonar_core::Result<Vec<_>>>()
        .context("looking up result paths")?;
    // Without --mode, a missing model is the normal text-only setup; with
    // it the user is told why CLAP did not run.
    let clap_asked = mode != SearchMode::Text && !query.trim().is_empty();
    let semantic = clap_asked && engine.semantic_available();
    let fallback = results.fallback.filter(|_| args.mode.is_some() || semantic);
    if let (Some(why), true) = (&fallback, args.mode.is_some() && !semantic && clap_asked) {
        eprintln!("note: text search only: {why}");
    }
    let report = Report {
        query: &query,
        mode: results.mode,
        fallback,
        total: results.text_total,
        semantic_candidates: results.semantic_candidates,
        took_ms,
        hits,
        facets: results.facets,
    };
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_text(&report);
        if let Some(f) = &report.facets {
            print_facets(f, &store)?;
        }
    }
    Ok(())
}

/// Facet counts, one line per dimension (`category: kick 12, snare 9`).
fn print_facets(f: &Facets, store: &Store) -> Result<()> {
    let line = |name: &str, counts: &[FacetCount]| {
        let items: Vec<String> = counts
            .iter()
            .map(|c| format!("{} {}", c.value, c.count))
            .collect();
        println!("{name:>10}: {}", items.join(", "));
    };
    line("category", &f.categories);
    line("kind", &f.kinds);
    line("key", &f.keys);
    let mut libs = Vec::new();
    for l in &f.libraries {
        let root = store
            .library(l.library_id)
            .context("looking up libraries")?
            .map_or_else(|| "?".into(), |lib| lib.root_path.display().to_string());
        libs.push(format!("#{} {root} {}", l.library_id, l.count));
    }
    println!("{:>10}: {}", "library", libs.join(", "));
    line("tag", &f.tags);
    println!("{:>10}: {}", "favorites", f.favorites);
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
    let fallback = r
        .fallback
        .as_deref()
        .map_or_else(String::new, |f| format!(", {f}"));
    match r.mode {
        SearchMode::Text => println!(
            "{} of {} results ({:.1} ms, text{fallback})",
            r.hits.len(),
            r.total,
            r.took_ms
        ),
        mode => println!(
            "{} results ({:.1} ms, {}: {} name matches, {} files with CLAP embeddings)",
            r.hits.len(),
            r.took_ms,
            mode.as_str(),
            r.total,
            r.semantic_candidates
        ),
    }
}
