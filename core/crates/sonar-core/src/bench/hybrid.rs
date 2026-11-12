//! Hybrid search latency (`sonar bench search --hybrid`, T5.4, ADR-029;
//! budget p95 < 50 ms at 500k files).
//!
//! Runs on the corpus and on-disk text index of [`super::search`]. Setup
//! (untimed): one clustered random 512-d unit row per indexed file (a
//! family centroid plus noise, like real CLAP rows of a sample library)
//! appended to an on-disk CLAP embedding store, re-opened read-only as
//! [`SemanticIndex`]. Queries go through [`hybrid_search`] end to end:
//! filters → allowed set (minus exclusions), BM25 leg with facets, query
//! embedding, exact cosine kNN, RRF.
//!
//! The text tower is the real CLAP one when the model is installed
//! (`encoder = "clap"`; also timed alone as `encode`), else a stand-in
//! that hashes words to a unit vector (`"synthetic"`, ≈ 0 ms: add the
//! ADR-026 figure of ≈ 5 ms per query).

use std::path::Path;
use std::time::Instant;

use rayon::prelude::*;
use serde::Serialize;

use super::search::{ms, percentile};
use super::search_corpus::{file_row, pack_and_family, Rng, FAMILIES};
use crate::embed::{EmbedOptions, EmbedStore, Embedding, TextEncoder, EMBED_DIM};
use crate::index::TextIndex;
use crate::search::{
    hybrid_search, Filters, HybridOptions, QueryEmbedder, SearchMode, SearchOptions, SemanticIndex,
    SemanticSearch, Synonyms, ValueRange,
};
use crate::{Error, Result};

/// Rows per append.
const BATCH: usize = 8192;
/// Spread of a file around its family centroid (centroid entries ~ 1).
const FILE_SPREAD: f32 = 0.9;

/// Whole queries (hybrid / semantic classes).
const QUERIES: [&str; 12] = [
    "dark kick",
    "punchy snare",
    "warm analog pad",
    "vocal chop",
    "808 glide",
    "lofi drums loop",
    "atmospheric texture",
    "bright pluck",
    "c minor bass",
    "riser",
    "kick -vinyl",
    "hh open",
];

/// Typed char by char (every prefix is one hybrid query).
const TYPED: [&str; 4] = ["dark kick", "warm pad", "snare -rim", "vocal chop"];

/// Latency of one class of hybrid queries.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HybridClassLatency {
    /// Class name.
    pub class: &'static str,
    /// Timed queries.
    pub n: usize,
    /// Median, ms.
    pub p50_ms: f64,
    /// 95th percentile (nearest rank), ms.
    pub p95_ms: f64,
    /// 99th percentile, ms.
    pub p99_ms: f64,
    /// Slowest, ms.
    pub max_ms: f64,
    /// Mean files scored by the semantic leg.
    pub mean_candidates: f64,
}

/// Result of the hybrid part of the search benchmark.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HybridBench {
    /// CLAP rows stored.
    pub vector_rows: usize,
    /// Bytes of the CLAP store.
    pub vector_bytes: u64,
    /// Writing the rows (untimed setup), s.
    pub setup_secs: f64,
    /// `clap` (real text tower) or `synthetic`.
    pub encoder: &'static str,
    /// Text tower alone per query (real encoder only), ms.
    pub encode: Option<HybridClassLatency>,
    /// End-to-end latency per class.
    pub classes: Vec<HybridClassLatency>,
}

/// Hashes words to a unit vector (no model needed).
struct HashEncoder;

impl QueryEmbedder for HashEncoder {
    fn embed_query(&mut self, text: &str) -> Result<Embedding> {
        let seed = text.bytes().fold(0xCBF2_9CE4_8422_2325u64, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x100_0000_01B3)
        });
        let mut r = Rng::new(seed, 0);
        Ok(normalize(std::array::from_fn(|_| uniform(&mut r))))
    }
}

/// In [-1, 1).
fn uniform(r: &mut Rng) -> f32 {
    (r.next() >> 40) as f32 / (1u64 << 23) as f32 - 1.0
}

fn normalize(mut e: Embedding) -> Embedding {
    let n = e.iter().map(|v| v * v).sum::<f32>().sqrt().max(1e-12);
    e.iter_mut().for_each(|v| *v /= n);
    e
}

/// Row of file number `i`: its family centroid plus noise, normalised.
fn row(centroids: &[Embedding], seed: u64, i: usize) -> Embedding {
    let (_, family) = pack_and_family(i);
    let mut r = Rng::new(seed ^ 0xC1A9, i);
    let c = &centroids[family % centroids.len()];
    normalize(std::array::from_fn(|d| {
        c[d] + FILE_SPREAD * uniform(&mut r)
    }))
}

/// Writes the rows, then times the hybrid classes over `ix` (the search
/// benchmark's index of `docs` files with `seed`) in `work`.
///
/// # Errors
/// Store, index and query errors.
pub(crate) fn bench_hybrid(
    work: &Path,
    ix: &TextIndex,
    docs: usize,
    seed: u64,
    rounds: usize,
    libraries: &[i64],
) -> Result<HybridBench> {
    let t = Instant::now();
    let dir = work.join("index").join("clap");
    let mut cr = Rng::new(seed ^ 0xCE47, 0);
    let centroids: Vec<Embedding> = (0..FAMILIES)
        .map(|_| normalize(std::array::from_fn(|_| uniform(&mut cr))))
        .collect();
    let mut vector_rows = 0;
    {
        let (mut store, _) = EmbedStore::open(&dir)?;
        for start in (0..docs).step_by(BATCH) {
            let rows: Vec<(i64, Embedding)> = (start..(start + BATCH).min(docs))
                .into_par_iter()
                .filter(|&i| !file_row(seed, i).missing)
                .map(|i| (i as i64 + 1, row(&centroids, seed, i)))
                .collect();
            let batch: Vec<(i64, &Embedding)> = rows.iter().map(|(id, e)| (*id, e)).collect();
            store.append(&batch)?;
            vector_rows += batch.len();
        }
    }
    let setup_secs = t.elapsed().as_secs_f64();
    let index = SemanticIndex::open(&dir, None)?;
    let vector_bytes = (vector_rows * (EMBED_DIM * 4 + 8)) as u64;

    let opts = EmbedOptions::default();
    let real = opts
        .unavailable_reason()
        .is_none()
        .then(|| TextEncoder::new(&opts).ok())
        .flatten();
    let (boxed, encoder, encode): (Box<dyn QueryEmbedder>, _, _) = match real {
        Some(mut enc) => {
            let mut times = Vec::new();
            for _ in 0..rounds.max(1) {
                for q in QUERIES {
                    let t = Instant::now();
                    enc.embed(q)?;
                    times.push((ms(t.elapsed()), 0));
                }
            }
            (Box::new(enc), "clap", Some(stats("encode", times)))
        }
        None => (Box::new(HashEncoder), "synthetic", None),
    };
    let sem = SemanticSearch::new(boxed, index);
    let mix = cases(libraries);
    let synonyms = Synonyms::builtin();
    let run = |c: &Case| -> Result<(f64, usize)> {
        let t = Instant::now();
        let r = hybrid_search(ix, &synonyms, Some(&sem), &c.query, &c.opts)?;
        let took = ms(t.elapsed());
        if r.mode != c.opts.mode {
            return Err(Error::Internal {
                reason: format!("{:?} ran as {:?}: {:?}", c.query, r.mode, r.fallback),
            });
        }
        Ok((took, r.semantic_candidates))
    };
    for c in &mix {
        run(c)?;
    }
    let mut runs: Vec<Vec<(f64, usize)>> = vec![Vec::new(); CLASSES.len()];
    for _ in 0..rounds {
        for c in &mix {
            runs[c.class].push(run(c)?);
        }
    }
    let classes = CLASSES
        .iter()
        .zip(runs)
        .map(|(name, times)| stats(name, times))
        .collect();
    Ok(HybridBench {
        vector_rows,
        vector_bytes,
        setup_secs,
        encoder,
        encode,
        classes,
    })
}

/// Class names, in report order.
const CLASSES: [&str; 4] = ["hybrid_typed", "hybrid_nl", "hybrid_filters", "semantic_nl"];

/// One timed query.
struct Case {
    /// Index into [`CLASSES`].
    class: usize,
    query: String,
    opts: HybridOptions,
}

/// The mix: every keystroke of [`TYPED`] (hybrid), [`QUERIES`] hybrid,
/// with filters, and semantic only; facets on as in the text mix.
fn cases(libraries: &[i64]) -> Vec<Case> {
    let opts = |mode, filters| HybridOptions {
        mode,
        search: SearchOptions {
            facets: true,
            filters,
            ..SearchOptions::default()
        },
        cancel: None,
    };
    let mut out = Vec::new();
    for q in TYPED {
        for (i, _) in q.char_indices().skip(1).chain([(q.len(), ' ')]) {
            let prefix = q[..i].trim_end();
            // Typed exclusion marks alone are not queries with words.
            if !prefix.ends_with('-') {
                out.push(Case {
                    class: 0,
                    query: q[..i].to_owned(),
                    opts: opts(SearchMode::Hybrid, Filters::default()),
                });
            }
        }
    }
    let filter_sets = [
        Filters {
            categories: vec!["kick".into(), "snare".into()],
            ..Filters::default()
        },
        Filters {
            bpm: Some(ValueRange::new(Some(120.0), Some(128.0))),
            kinds: vec!["loop".into()],
            ..Filters::default()
        },
        Filters {
            libraries: libraries.iter().take(2).copied().collect(),
            ..Filters::default()
        },
    ];
    for (i, q) in QUERIES.iter().enumerate() {
        let query = (*q).to_owned();
        out.push(Case {
            class: 1,
            query: query.clone(),
            opts: opts(SearchMode::Hybrid, Filters::default()),
        });
        out.push(Case {
            class: 2,
            query: query.clone(),
            opts: opts(
                SearchMode::Hybrid,
                filter_sets[i % filter_sets.len()].clone(),
            ),
        });
        out.push(Case {
            class: 3,
            query,
            opts: opts(SearchMode::Semantic, Filters::default()),
        });
    }
    out
}

/// Percentiles of `(ms, candidates)` samples.
fn stats(class: &'static str, mut samples: Vec<(f64, usize)>) -> HybridClassLatency {
    samples.sort_by(|a, b| a.0.total_cmp(&b.0));
    let times: Vec<f64> = samples.iter().map(|s| s.0).collect();
    let n = times.len();
    HybridClassLatency {
        class,
        n,
        p50_ms: percentile(&times, 0.50),
        p95_ms: percentile(&times, 0.95),
        p99_ms: percentile(&times, 0.99),
        max_ms: times.last().copied().unwrap_or(0.0),
        mean_candidates: samples.iter().map(|s| s.1 as f64).sum::<f64>() / n.max(1) as f64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mix_types_every_prefix_and_rows_cluster() {
        let mix = cases(&[1, 2]);
        let typed: Vec<&str> = mix
            .iter()
            .filter(|c| c.class == 0)
            .map(|c| c.query.as_str())
            .take(9)
            .collect();
        assert_eq!(
            typed,
            [
                "d",
                "da",
                "dar",
                "dark",
                "dark ",
                "dark k",
                "dark ki",
                "dark kic",
                "dark kick"
            ]
        );
        assert!(!mix.iter().any(|c| c.query.trim_end().ends_with('-')));
        for class in 0..CLASSES.len() {
            assert!(mix.iter().any(|c| c.class == class));
        }
        let mut r = Rng::new(1, 0);
        let centroids: Vec<Embedding> = (0..FAMILIES)
            .map(|_| normalize(std::array::from_fn(|_| uniform(&mut r))))
            .collect();
        let dot = |a: &Embedding, b: &Embedding| a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>();
        let a = row(&centroids, 1, 0);
        assert!((dot(&a, &a) - 1.0).abs() < 1e-4);
        // Same leaf folder (same family) vs another family.
        assert!(dot(&a, &row(&centroids, 1, 1)) > dot(&a, &row(&centroids, 1, 60)));
        let mut h = HashEncoder;
        assert_eq!(h.embed_query("kick").ok(), h.embed_query("kick").ok());
    }
}
