//! `sonar index` (rebuild the text index) and `sonar search` (query it),
//! plus the refresh `sonar scan` runs after scanning.

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result};
use serde::Serialize;
use sonar_core::embed::{store::dir_for_db, EmbedOptions};
use sonar_core::index::{OpenState, TextIndex};
use sonar_core::search::{
    embedded_file_ids, hybrid_search, FacetCount, Facets, HybridOptions, SearchMode, SearchOptions,
    SemanticSearch, Synonyms,
};
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
/// from another version) and print rank, score, file id and path.
pub fn run(args: &Args<'_>) -> Result<()> {
    let store = Store::open_default().context("opening the Sonar store")?;
    let ix = open_index(&store)?;
    let synonyms = match args.synonyms {
        Some(path) => {
            Synonyms::load(path).with_context(|| format!("reading synonyms {}", path.display()))?
        }
        None => Synonyms::load_default().context("reading synonyms")?,
    };
    let query = args.query.join(" ");
    let opts = HybridOptions {
        mode: args.mode.unwrap_or_default(),
        search: SearchOptions {
            limit: args.limit,
            prefix: !args.no_prefix,
            filters: args.filters.flags.filters(&store)?,
            sort: args.filters.sort.unwrap_or_default(),
            facets: args.filters.facets,
        },
        cancel: None,
    };
    let semantic = if opts.mode == SearchMode::Text || query.trim().is_empty() {
        None
    } else {
        load_semantic(&store, args.mode.is_some())
    };
    let started = Instant::now();
    let results =
        hybrid_search(&ix, &synonyms, semantic.as_ref(), &query, &opts).context("searching")?;
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
    // Without --mode, a missing model is the normal text-only setup.
    let fallback = results
        .fallback
        .filter(|_| args.mode.is_some() || semantic.is_some());
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

/// The CLAP text tower plus the current embeddings of the store's files,
/// or `None`, with a note on stderr when the user asked for a CLAP mode
/// (without `--mode`, no model or no embeddings is the normal text-only
/// setup). The text tower is not loaded when no file has an embedding.
fn load_semantic(store: &Store, explicit: bool) -> Option<SemanticSearch> {
    let opts = EmbedOptions::default();
    let loaded = (|| -> Result<SemanticSearch> {
        if let Some(reason) = opts.unavailable_reason() {
            anyhow::bail!(reason);
        }
        let db = store
            .path()
            .context("the store is in memory (no embeddings)")?;
        let current = embedded_file_ids(store).context("reading embed results")?;
        if current.is_empty() {
            anyhow::bail!("no CLAP embeddings yet (run `sonar analyze --stages meta,dsp,embed`)");
        }
        SemanticSearch::load(&opts, &dir_for_db(db), Some(&current)).context("loading CLAP search")
    })();
    match loaded {
        Ok(s) => Some(s),
        Err(e) => {
            if explicit {
                eprintln!("note: text search only: {e:#}");
            }
            None
        }
    }
}

/// Opens the default text index, building it from `store` first if it is
/// missing or from another version (says so on stderr).
pub fn open_index(store: &Store) -> Result<TextIndex> {
    let (ix, state) = TextIndex::open_default().context("opening the search index")?;
    if let OpenState::NeedsRebuild(reason) = state {
        eprintln!("building the search index ({reason})...");
        let r = ix.rebuild(store).context("building the search index")?;
        eprintln!(
            "indexed {} files in {:.2} s",
            r.docs,
            r.elapsed.as_secs_f64()
        );
    }
    Ok(ix)
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
