//! Natural-language and hybrid search (T5.4, ADR-029, docs/ARCHITECTURE.md
//! §6): filters → BM25 text → CLAP text-embedding kNN → reciprocal-rank
//! fusion.
//!
//! - **Text leg**: [`search`] as is (ADR-017/018: every word narrows,
//!   synonyms, exclusions, as-you-type prefix, filters, facets), top
//!   `depth` hits.
//! - **Semantic leg**: the positive words of the query (exclusions and
//!   quotes removed) through the CLAP text tower, then exact cosine kNN
//!   ([`SemanticIndex::knn`]) over the files that pass the filters and are
//!   not matched by an exclusion ([`allowed_file_ids`]), top `depth`.
//! - **Fusion**: Reciprocal Rank Fusion (Cormack, Clarke & Büttcher 2009,
//!   "Reciprocal rank fusion outperforms Condorcet and individual rank
//!   learning methods", SIGIR): `score(d) = Σ_legs 1 / (k + rank_leg(d))`,
//!   rank from 1, `k` = [`RRF_K`] = 60 as in the paper. Rank-only, so BM25
//!   and cosine scales never need calibrating. Ties: better best rank,
//!   then file id.
//!
//! Modes ([`SearchMode`]): `Text` is [`search`]; `Semantic` is the
//! semantic leg alone (cosine order); `Hybrid` fuses both. A query without
//! positive words (browse, only exclusions), a sort other than relevance
//! (the semantic leg has no match set to sort), or no semantic resources
//! (build without the `embed` feature, model missing, no embeddings yet,
//! text tower error) run as `Text`, with [`HybridResults::fallback`]
//! saying why; never an error.
//!
//! Facets: `Hybrid` returns the text leg's counts (its matching set);
//! `Semantic` none (it has no match set).

use std::collections::HashMap;
use std::path::Path;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;

use super::parse::parse;
use super::semantic::{SemanticHit, SemanticIndex};
use super::{allowed_file_ids, search, Facets, SearchOptions, SortField, Synonyms};
use crate::embed::{EmbedOptions, Embedding, TextEncoder};
use crate::index::TextIndex;
use crate::{Error, Result};

/// RRF constant (Cormack et al. 2009).
pub const RRF_K: f64 = 60.0;
/// Hits taken from each leg before fusion (at least the requested limit).
pub const RRF_DEPTH: usize = 200;

/// Which legs a query runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchMode {
    /// BM25 and CLAP fused by RRF.
    #[default]
    Hybrid,
    /// BM25 only (ADR-017).
    Text,
    /// CLAP text-to-audio only.
    Semantic,
}

impl SearchMode {
    /// Name for the CLI and reports.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hybrid => "hybrid",
            Self::Text => "text",
            Self::Semantic => "semantic",
        }
    }
}

impl FromStr for SearchMode {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "hybrid" => Ok(Self::Hybrid),
            "text" => Ok(Self::Text),
            "semantic" => Ok(Self::Semantic),
            other => Err(format!(
                "unknown search mode '{other}' (hybrid, text or semantic)"
            )),
        }
    }
}

/// Turns query text into a CLAP text embedding (the real text tower, or a
/// stand-in in tests and benchmarks).
pub trait QueryEmbedder: Send {
    /// L2-normalised embedding of `text`.
    ///
    /// # Errors
    /// Whatever the encoder reports; the caller falls back to text search.
    fn embed_query(&mut self, text: &str) -> Result<Embedding>;
}

impl QueryEmbedder for TextEncoder {
    fn embed_query(&mut self, text: &str) -> Result<Embedding> {
        self.embed(text)
    }
}

/// What the semantic leg needs, loaded once and shared by queries: the
/// text encoder (one query at a time behind a mutex; ≈ 5 ms) and the
/// audio embeddings.
pub struct SemanticSearch {
    encoder: Mutex<Box<dyn QueryEmbedder>>,
    index: SemanticIndex,
}

impl std::fmt::Debug for SemanticSearch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SemanticSearch")
            .field("index", &self.index)
            .finish_non_exhaustive()
    }
}

impl SemanticSearch {
    /// From an encoder and an index.
    pub fn new(encoder: Box<dyn QueryEmbedder>, index: SemanticIndex) -> Self {
        Self {
            encoder: Mutex::new(encoder),
            index,
        }
    }

    /// Loads the CLAP text tower of `opts` and the embedding store in
    /// `store_dir`, restricted to the `current` files (see
    /// [`super::semantic::embedded_file_ids`]).
    ///
    /// # Errors
    /// [`Error::ModelMissing`] / [`Error::Embed`] (no model, no `embed`
    /// feature), [`Error::VectorStore`] (no store yet), [`Error::Io`].
    pub fn load(opts: &EmbedOptions, store_dir: &Path, current: Option<&[i64]>) -> Result<Self> {
        let index = SemanticIndex::open(store_dir, current)?;
        let encoder = TextEncoder::new(opts)?;
        Ok(Self::new(Box::new(encoder), index))
    }

    /// The audio embeddings.
    pub fn index(&self) -> &SemanticIndex {
        &self.index
    }

    /// Text embedding of `text`.
    ///
    /// # Errors
    /// The encoder's error; [`Error::Internal`] if a previous query
    /// panicked inside the encoder.
    pub fn embed(&self, text: &str) -> Result<Embedding> {
        let mut enc = self.encoder.lock().map_err(|_| Error::Internal {
            reason: "text encoder poisoned by an earlier panic".into(),
        })?;
        enc.embed_query(text)
    }
}

/// How to run a hybrid search.
#[derive(Debug, Clone, Default)]
pub struct HybridOptions {
    /// Legs to run.
    pub mode: SearchMode,
    /// Limit, prefix, filters, sort and facets (as for [`search`]).
    pub search: SearchOptions,
    /// Set to stop the query early ([`Error::Cancelled`]).
    pub cancel: Option<Arc<AtomicBool>>,
}

/// One result.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct HybridHit {
    /// `files.id`.
    pub file_id: i64,
    /// Hybrid: RRF score; text: BM25 (see [`super::Hit`]); semantic: cosine.
    pub score: f32,
    /// Rank in the text leg (from 1), if it was in its top `depth`.
    pub text_rank: Option<u32>,
    /// Rank in the semantic leg (from 1), if it was in its top `depth`.
    pub semantic_rank: Option<u32>,
}

/// Results of [`hybrid_search`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HybridResults {
    /// The legs that actually ran.
    pub mode: SearchMode,
    /// Why a hybrid / semantic request ran as text, if it did.
    pub fallback: Option<String>,
    /// Files the text leg matched (every match is counted).
    pub text_total: usize,
    /// Filtered files with an embedding the semantic leg scored.
    pub semantic_candidates: usize,
    /// Best hits, at most the requested limit.
    pub hits: Vec<HybridHit>,
    /// Facet counts, if asked (see the module docs).
    pub facets: Option<Facets>,
}

/// The query's positive words, as the text tower should read them
/// (exclusions dropped, quotes removed).
pub fn semantic_text(query: &str) -> String {
    let parsed = parse(query);
    let words: Vec<&str> = parsed
        .chunks
        .iter()
        .filter(|c| !c.negated)
        .map(|c| c.text.trim())
        .collect();
    words.join(" ")
}

/// Runs `query` in `opts.mode` (see the module docs).
///
/// # Errors
/// [`Error::Index`], [`Error::Cancelled`]; semantic failures fall back to
/// text instead.
pub fn hybrid_search(
    index: &TextIndex,
    synonyms: &Synonyms,
    semantic: Option<&SemanticSearch>,
    query: &str,
    opts: &HybridOptions,
) -> Result<HybridResults> {
    if opts.mode == SearchMode::Text {
        return text_only(index, synonyms, query, opts, None);
    }
    let text = semantic_text(query);
    let unavailable = if text.is_empty() {
        Some("no query words".to_owned())
    } else if opts.search.sort.field != SortField::Relevance {
        Some("sorted by a field".to_owned())
    } else {
        match semantic {
            None => Some("semantic search unavailable".to_owned()),
            Some(s) if s.index().is_empty() => Some("no CLAP embeddings yet".to_owned()),
            Some(_) => None,
        }
    };
    let Some(sem) = semantic.filter(|_| unavailable.is_none()) else {
        return text_only(index, synonyms, query, opts, unavailable);
    };
    let cancel = opts.cancel.as_deref();
    let depth = opts.search.limit.max(RRF_DEPTH);
    let hybrid = opts.mode == SearchMode::Hybrid;
    let text_opts = SearchOptions {
        limit: depth,
        ..opts.search.clone()
    };
    // The text leg runs while the query is embedded and the candidate set
    // is collected.
    let (text_leg, (embedded, allowed)) = rayon::join(
        || hybrid.then(|| search(index, synonyms, query, &text_opts)),
        || {
            rayon::join(
                || sem.embed(&text),
                || allowed_file_ids(index, synonyms, query, &opts.search.filters),
            )
        },
    );
    let text_leg = text_leg.transpose()?;
    let allowed = allowed?;
    let embedded = match embedded {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!(error = %e, "query embedding failed, text search only");
            let reason = Some(format!("text encoder failed: {e}"));
            return match text_leg {
                Some(t) => Ok(as_text(t, opts.search.limit, reason)),
                None => text_only(index, synonyms, query, opts, reason),
            };
        }
    };
    if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
        return Err(Error::Cancelled);
    }
    let (sem_hits, candidates) = sem.index().knn(&embedded, &allowed, depth, cancel)?;
    let Some(text_leg) = text_leg else {
        let hits = sem_hits
            .iter()
            .take(opts.search.limit)
            .zip(1u32..)
            .map(|(h, rank)| HybridHit {
                file_id: h.file_id,
                score: h.cosine,
                text_rank: None,
                semantic_rank: Some(rank),
            })
            .collect();
        return Ok(HybridResults {
            mode: SearchMode::Semantic,
            fallback: None,
            text_total: 0,
            semantic_candidates: candidates,
            hits,
            facets: None,
        });
    };
    let text_ids: Vec<i64> = text_leg.hits.iter().map(|h| h.file_id).collect();
    let mut hits = rrf(&text_ids, &sem_hits);
    hits.truncate(opts.search.limit);
    Ok(HybridResults {
        mode: SearchMode::Hybrid,
        fallback: None,
        text_total: text_leg.total,
        semantic_candidates: candidates,
        hits,
        facets: text_leg.facets,
    })
}

/// [`search`] with its results as [`HybridResults`].
fn text_only(
    index: &TextIndex,
    synonyms: &Synonyms,
    query: &str,
    opts: &HybridOptions,
    fallback: Option<String>,
) -> Result<HybridResults> {
    if opts
        .cancel
        .as_deref()
        .is_some_and(|c| c.load(Ordering::Relaxed))
    {
        return Err(Error::Cancelled);
    }
    let r = search(index, synonyms, query, &opts.search)?;
    Ok(as_text(r, opts.search.limit, fallback))
}

fn as_text(r: super::SearchResults, limit: usize, fallback: Option<String>) -> HybridResults {
    let hits = r
        .hits
        .iter()
        .take(limit)
        .zip(1u32..)
        .map(|(h, rank)| HybridHit {
            file_id: h.file_id,
            score: h.score,
            text_rank: Some(rank),
            semantic_rank: None,
        })
        .collect();
    HybridResults {
        mode: SearchMode::Text,
        fallback,
        text_total: r.total,
        semantic_candidates: 0,
        hits,
        facets: r.facets,
    }
}

/// Reciprocal Rank Fusion of the two ranked lists (module docs), best
/// first.
pub(crate) fn rrf(text: &[i64], semantic: &[SemanticHit]) -> Vec<HybridHit> {
    let mut fused: HashMap<i64, (f64, HybridHit)> =
        HashMap::with_capacity(text.len() + semantic.len());
    let sem_ids: Vec<i64> = semantic.iter().map(|h| h.file_id).collect();
    for (is_text, ids) in [(true, text), (false, &sem_ids[..])] {
        for (rank, &id) in (1u32..).zip(ids) {
            let (score, hit) = fused.entry(id).or_insert((
                0.0,
                HybridHit {
                    file_id: id,
                    score: 0.0,
                    text_rank: None,
                    semantic_rank: None,
                },
            ));
            let slot = if is_text {
                &mut hit.text_rank
            } else {
                &mut hit.semantic_rank
            };
            // A list never repeats an id; keep the first rank if it did.
            if slot.is_none() {
                *slot = Some(rank);
                *score += 1.0 / (RRF_K + f64::from(rank));
            }
        }
    }
    let best = |h: &HybridHit| {
        h.text_rank
            .unwrap_or(u32::MAX)
            .min(h.semantic_rank.unwrap_or(u32::MAX))
    };
    let mut out: Vec<(f64, HybridHit)> = fused.into_values().collect();
    out.sort_by(|(sa, a), (sb, b)| {
        sb.total_cmp(sa)
            .then(best(a).cmp(&best(b)))
            .then(a.file_id.cmp(&b.file_id))
    });
    out.into_iter()
        .map(|(s, mut h)| {
            h.score = s as f32;
            h
        })
        .collect()
}

#[cfg(test)]
#[path = "hybrid_tests.rs"]
mod tests;
