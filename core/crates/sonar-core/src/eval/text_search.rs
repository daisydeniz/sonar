//! Text / NL search metrics of `docs/EVAL.md` ("Text / NL search", T5.4,
//! ADR-029): NDCG@10 and recall@50 of [`hybrid_search`] in each mode.
//!
//! The evaluated files become a small in-RAM library: a text index whose
//! documents carry what analysis gives an unnamed file (DSP-only category,
//! kind, BPM, key; the same audio-only values as the other metrics) and,
//! when the CLAP model is present, a [`SemanticIndex`] of their audio
//! embeddings. Fixture names and folders are hidden (they carry the truth
//! group); golden files are indexed with their relative path, as the
//! product indexes a library. Queries run through the shipped
//! [`hybrid_search`] with default options.
//!
//! **NDCG@10** (Järvelin & Kekäläinen 2002): gain `2^grade − 1`, discount
//! `log2(rank + 1)`, normalised by the ideal ordering of every judged file
//! in the pool. **Recall@50**: relevant files in the top 50 over
//! `min(|relevant|, 50)`. Queries without a judged file in the pool do not
//! count. Both in percent, mean over queries.
//!
//! BM25 (text mode) numbers never depend on `models/` and are tracked;
//! semantic and hybrid numbers need the model and are report only.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::classify::{Category, Kind};
use crate::embed::{Embedding, TextEncoder};
use crate::index::{IndexDoc, TextIndex};
use crate::search::hybrid::semantic_text;
use crate::search::{
    hybrid_search, HybridOptions, QueryEmbedder, SearchMode, SearchOptions, SemanticIndex,
    SemanticSearch, Synonyms,
};
use crate::text::Key;
use crate::{Error, Result};

use super::text_queries::TextQuery;

/// Rank cut-off of NDCG.
pub const NDCG_K: usize = 10;
/// Rank cut-off of recall (also the number of hits asked for).
pub const RECALL_K: usize = 50;

/// One evaluated file as the search library sees it.
#[derive(Debug, Clone)]
pub struct Entry {
    /// File (identity for the judgments).
    pub path: PathBuf,
    /// Relative path indexed as name + folders (`None` = hidden).
    pub rel_path: Option<String>,
    /// Detected kind.
    pub kind: Option<Kind>,
    /// Detected tempo.
    pub bpm: Option<f64>,
    /// Detected key.
    pub key: Option<Key>,
    /// DSP-only category.
    pub category: Option<Category>,
    /// CLAP audio embedding (model present).
    pub embedding: Option<Embedding>,
}

/// Mean scores of one mode, percent.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct ModeScores {
    /// NDCG@10.
    pub ndcg10_pct: f64,
    /// Recall@50.
    pub recall50_pct: f64,
}

/// NDCG@10 of one query per mode (report detail).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueryRow {
    /// Query text.
    pub query: String,
    /// Judged files in the pool.
    pub relevant: usize,
    /// Text (BM25) mode.
    pub bm25: f64,
    /// Semantic mode.
    pub semantic: Option<f64>,
    /// Hybrid mode.
    pub hybrid: Option<f64>,
}

/// Text-search metrics of a tier.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TextSearchMetrics {
    /// Queries with at least one judged file in the pool.
    pub queries: usize,
    /// Files in the library.
    pub pool: usize,
    /// Queries without a judged file in the pool, plus judged files not in
    /// the pool.
    pub skipped: usize,
    /// Text mode (BM25, ADR-017).
    pub bm25: ModeScores,
    /// CLAP only (model present).
    pub semantic: Option<ModeScores>,
    /// RRF of both (model present).
    pub hybrid: Option<ModeScores>,
    /// Per query NDCG@10.
    pub per_query: Vec<QueryRow>,
}

/// Looks query embeddings up in a table filled before the run.
struct Precomputed(HashMap<String, Embedding>);

impl QueryEmbedder for Precomputed {
    fn embed_query(&mut self, text: &str) -> Result<Embedding> {
        self.0.get(text).copied().ok_or_else(|| Error::Embed {
            reason: format!("query {text:?} was not embedded"),
        })
    }
}

/// DCG of `gains` in order (first = rank 1).
fn dcg(gains: impl Iterator<Item = u8>) -> f64 {
    gains
        .enumerate()
        .map(|(i, g)| (2f64.powi(i32::from(g)) - 1.0) / ((i + 2) as f64).log2())
        .fold(0.0, |a, b| a + b)
}

/// NDCG@k of `ranked` pool positions under `grades` (0..1).
pub fn ndcg_at(ranked: &[usize], grades: &HashMap<usize, u8>, k: usize) -> f64 {
    let mut ideal: Vec<u8> = grades.values().copied().collect();
    ideal.sort_unstable_by(|a, b| b.cmp(a));
    let idcg = dcg(ideal.into_iter().take(k));
    if idcg <= 0.0 {
        return 0.0;
    }
    let got = ranked
        .iter()
        .take(k)
        .map(|p| grades.get(p).copied().unwrap_or(0));
    dcg(got) / idcg
}

/// Recall@k with the `min(|relevant|, k)` denominator (0..1).
pub fn recall_at(ranked: &[usize], grades: &HashMap<usize, u8>, k: usize) -> f64 {
    let want = grades.len().min(k);
    if want == 0 {
        return 0.0;
    }
    let found = ranked
        .iter()
        .take(k)
        .filter(|p| grades.contains_key(p))
        .count();
    found as f64 / want as f64
}

fn doc(pos: usize, e: &Entry) -> IndexDoc {
    let file_id = pos as i64 + 1;
    let base = match &e.rel_path {
        Some(rel) => IndexDoc::from_paths(file_id, Path::new(""), rel),
        None => IndexDoc {
            file_id,
            ..IndexDoc::default()
        },
    };
    IndexDoc {
        category: e.category.map(|c| c.as_str().to_owned()),
        kind: e.kind.map(|k| {
            match k {
                Kind::Loop => "loop",
                Kind::OneShot => "one_shot",
            }
            .to_owned()
        }),
        bpm: e.bpm,
        key: e.key,
        ..base
    }
}

/// Scores `queries` over `entries` (module docs). `encoder` enables the
/// semantic and hybrid modes when entries have embeddings. `None` when no
/// query has a judged file in the pool.
///
/// # Errors
/// [`Error::Index`] building the in-RAM index or searching.
pub fn compute(
    entries: &[Entry],
    queries: &[TextQuery],
    encoder: Option<&Mutex<TextEncoder>>,
) -> Result<Option<TextSearchMetrics>> {
    let ix = TextIndex::in_ram()?;
    let docs: Vec<IndexDoc> = entries.iter().enumerate().map(|(i, e)| doc(i, e)).collect();
    ix.upsert(&docs)?;
    let by_path: HashMap<&Path, usize> = entries
        .iter()
        .enumerate()
        .map(|(i, e)| (e.path.as_path(), i))
        .collect();
    let semantic = semantic(entries, queries, encoder);
    let synonyms = Synonyms::builtin();
    let run = |q: &str, mode, sem: Option<&SemanticSearch>| -> Result<Vec<usize>> {
        let opts = HybridOptions {
            mode,
            search: SearchOptions {
                limit: RECALL_K,
                ..SearchOptions::default()
            },
            cancel: None,
        };
        let r = hybrid_search(&ix, &synonyms, sem, q, &opts)?;
        Ok(r.hits
            .iter()
            .filter_map(|h| usize::try_from(h.file_id - 1).ok())
            .collect())
    };
    let mut m = TextSearchMetrics {
        pool: entries.len(),
        ..TextSearchMetrics::default()
    };
    let (mut bm25, mut sem_sum, mut hyb_sum) = (
        ModeScores::default(),
        ModeScores::default(),
        ModeScores::default(),
    );
    let add = |acc: &mut ModeScores, ranked: &[usize], g: &HashMap<usize, u8>| {
        let n = ndcg_at(ranked, g, NDCG_K);
        acc.ndcg10_pct += n;
        acc.recall50_pct += recall_at(ranked, g, RECALL_K);
        n * 100.0
    };
    for q in queries {
        let mut grades = HashMap::new();
        for (p, g) in &q.relevant {
            match by_path.get(p.as_path()) {
                Some(&i) => {
                    grades.insert(i, *g);
                }
                None => m.skipped += 1,
            }
        }
        if grades.is_empty() {
            m.skipped += 1;
            continue;
        }
        m.queries += 1;
        let text = add(&mut bm25, &run(&q.query, SearchMode::Text, None)?, &grades);
        let mut row = QueryRow {
            query: q.query.clone(),
            relevant: grades.len(),
            bm25: text,
            semantic: None,
            hybrid: None,
        };
        if let Some(s) = &semantic {
            let ranked = run(&q.query, SearchMode::Semantic, Some(s))?;
            row.semantic = Some(add(&mut sem_sum, &ranked, &grades));
            let ranked = run(&q.query, SearchMode::Hybrid, Some(s))?;
            row.hybrid = Some(add(&mut hyb_sum, &ranked, &grades));
        }
        m.per_query.push(row);
    }
    if m.queries == 0 {
        return Ok(None);
    }
    let mean = |s: ModeScores| ModeScores {
        ndcg10_pct: 100.0 * s.ndcg10_pct / m.queries as f64,
        recall50_pct: 100.0 * s.recall50_pct / m.queries as f64,
    };
    m.bm25 = mean(bm25);
    if semantic.is_some() {
        m.semantic = Some(mean(sem_sum));
        m.hybrid = Some(mean(hyb_sum));
    }
    Ok(Some(m))
}

/// The semantic resources: every entry's embedding and every query's text
/// embedding (`None` without the encoder, embeddings, or on an encoder
/// error).
fn semantic(
    entries: &[Entry],
    queries: &[TextQuery],
    encoder: Option<&Mutex<TextEncoder>>,
) -> Option<SemanticSearch> {
    let rows: Vec<(i64, Embedding)> = entries
        .iter()
        .enumerate()
        .filter_map(|(i, e)| Some((i as i64 + 1, e.embedding?)))
        .collect();
    if rows.is_empty() {
        return None;
    }
    let mut enc = encoder?.lock().ok()?;
    let mut table = HashMap::new();
    for q in queries {
        let text = semantic_text(&q.query);
        if !text.is_empty() && !table.contains_key(&text) {
            let e = enc.embed(&text).ok()?;
            table.insert(text, e);
        }
    }
    Some(SemanticSearch::new(
        Box::new(Precomputed(table)),
        SemanticIndex::from_embeddings(rows),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ndcg_and_recall_by_hand() {
        let grades: HashMap<usize, u8> = [(1, 2), (2, 1)].into_iter().collect();
        // Ideal: 3/log2(2) + 1/log2(3).
        let ideal = 3.0 + 1.0 / 3f64.log2();
        assert!((ndcg_at(&[1, 2, 7], &grades, 10) - 1.0).abs() < 1e-12);
        let swapped = (1.0 + 3.0 / 3f64.log2()) / ideal;
        assert!((ndcg_at(&[2, 1], &grades, 10) - swapped).abs() < 1e-12);
        assert!((ndcg_at(&[9, 1], &grades, 10) - (3.0 / 3f64.log2()) / ideal).abs() < 1e-12);
        assert_eq!(ndcg_at(&[1, 2], &grades, 0), 0.0);
        assert_eq!(ndcg_at(&[1], &HashMap::new(), 10), 0.0);
        assert_eq!(recall_at(&[1, 5], &grades, 50), 0.5);
        assert_eq!(recall_at(&[5, 2, 1], &grades, 2), 0.5);
        let many: HashMap<usize, u8> = (0..80).map(|i| (i, 1)).collect();
        let top: Vec<usize> = (0..50).collect();
        assert_eq!(recall_at(&top, &many, 50), 1.0);
    }

    fn entry(n: usize, category: Category, kind: Kind) -> Entry {
        Entry {
            path: PathBuf::from(format!("/f/{n}.wav")),
            rel_path: None,
            kind: Some(kind),
            bpm: None,
            key: None,
            category: Some(category),
            embedding: None,
        }
    }

    #[test]
    fn bm25_finds_files_by_their_detected_category_only() {
        let entries = vec![
            entry(0, Category::Kick, Kind::OneShot),
            entry(1, Category::Snare, Kind::OneShot),
            entry(2, Category::Kick, Kind::OneShot),
            entry(3, Category::Drums, Kind::Loop),
        ];
        let judged = |q: &str, files: &[usize]| TextQuery {
            query: q.into(),
            relevant: files
                .iter()
                .map(|n| (PathBuf::from(format!("/f/{n}.wav")), 2))
                .collect(),
        };
        let queries = [
            judged("kick", &[0, 2]),
            judged("snare", &[1, 3]),
            judged("tuba", &[9]),
        ];
        let m = compute(&entries, &queries, None)
            .expect("run")
            .expect("metrics");
        assert_eq!((m.queries, m.pool, m.skipped), (2, 4, 2));
        // "kick": both kicks found (NDCG 1); "snare": 1 of 2 at rank 1.
        assert!((m.per_query[0].bm25 - 100.0).abs() < 1e-9);
        assert!((m.bm25.recall50_pct - 75.0).abs() < 1e-9);
        assert!(m.semantic.is_none() && m.hybrid.is_none());
        assert!(compute(&entries, &queries[2..], None)
            .expect("run")
            .is_none());
    }
}
