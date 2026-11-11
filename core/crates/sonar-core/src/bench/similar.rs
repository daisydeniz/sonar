//! Similarity latency benchmark (`sonar bench similar`, T4.3, ADR-025;
//! budget: p95 < 50 ms at 500k files, docs/PRODUCT.md).
//!
//! 1. Writes the synthetic library of the search benchmark into a fresh
//!    store, builds its on-disk text index by the production path, and
//!    appends one clustered random aspect row per indexed file to an
//!    on-disk vector store ([`super::similar_corpus`]); all untimed.
//! 2. Re-opens the vector store read-only, as the app queries it.
//! 3. Runs `queries` similarity queries per [`SimilarClass`] (interleaved,
//!    after an untimed warm-up) through the real end-to-end
//!    [`similar`] call (seed row, filter → allowed ids, scoring, top-K),
//!    then times the two phases of the same query on their own:
//!    [`filtered_file_ids`] and [`rank`] (the scoring scan).

use std::path::Path;
use std::time::Instant;

use serde::Serialize;

use super::search::{ms, percentile, DEFAULT_SEED, SEARCH_BUDGET_MS};
use super::search_corpus::{file_row, generate_store, Rng, SearchCorpus};
use super::similar_corpus::write_vectors;
use super::{machine, Machine};
use crate::dsp::aspects::Aspect;
use crate::index::vectors::VectorStore;
use crate::index::TextIndex;
use crate::search::{filtered_file_ids, Filters, ValueRange};
use crate::similar::{rank, seed_vectors, similar, AspectWeights, Seed, SimilarOptions};
use crate::{Error, Result, Store};

/// What to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SimilarBenchOptions {
    /// Files in the synthetic library.
    pub docs: usize,
    /// Timed queries per class.
    pub queries: usize,
    /// Corpus seed.
    pub seed: u64,
    /// Threads of the pool the queries run in (0 = all cores, the app's
    /// global pool size).
    pub threads: usize,
}

impl Default for SimilarBenchOptions {
    fn default() -> Self {
        Self {
            docs: 500_000,
            queries: 60,
            seed: DEFAULT_SEED,
            threads: 0,
        }
    }
}

/// Kinds of similarity query, reported separately.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SimilarClass {
    /// Default (Overall) weights, default filters (hidden files excluded).
    Overall,
    /// Custom aspect weights (mixed, single aspect), default filters.
    Weighted,
    /// Default weights with a category / kind / library / BPM filter.
    Filtered,
    /// No filter clause at all (hidden files included).
    Unfiltered,
}

impl SimilarClass {
    /// Every class, in report order.
    pub const ALL: [Self; 4] = [
        Self::Overall,
        Self::Weighted,
        Self::Filtered,
        Self::Unfiltered,
    ];

    /// Short name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Overall => "overall",
            Self::Weighted => "weighted",
            Self::Filtered => "filtered",
            Self::Unfiltered => "unfiltered",
        }
    }
}

/// Latency percentiles of one phase, ms.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct Percentiles {
    /// Median.
    pub p50_ms: f64,
    /// 95th percentile (nearest rank).
    pub p95_ms: f64,
    /// Slowest.
    pub max_ms: f64,
}

impl Percentiles {
    fn of(mut times: Vec<f64>) -> Self {
        times.sort_by(f64::total_cmp);
        Self {
            p50_ms: percentile(&times, 0.50),
            p95_ms: percentile(&times, 0.95),
            max_ms: times.last().copied().unwrap_or(0.0),
        }
    }
}

/// Latency of one query class.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SimilarClassLatency {
    /// Class name.
    pub class: &'static str,
    /// Timed queries.
    pub n: usize,
    /// End-to-end [`similar`] call.
    pub total: Percentiles,
    /// [`filtered_file_ids`] alone.
    pub filter: Percentiles,
    /// [`rank`] alone (the scoring scan over the allowed ids).
    pub scoring: Percentiles,
    /// Mean candidates scored per query.
    pub mean_candidates: f64,
}

/// Result of [`bench_similar`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SimilarBench {
    /// Where it ran.
    pub machine: Machine,
    /// Threads of the rayon pool the scan may use.
    pub threads: usize,
    /// File rows generated.
    pub docs: usize,
    /// Vector rows stored (indexed files).
    pub vector_rows: usize,
    /// Bytes of the vector store files.
    pub vector_bytes: u64,
    /// Corpus seed.
    pub seed: u64,
    /// Store rows, text index and vectors (untimed setup), s.
    pub setup_secs: f64,
    /// Hits asked per query.
    pub limit: usize,
    /// Per-class latency, in [`SimilarClass::ALL`] order.
    pub classes: Vec<SimilarClassLatency>,
    /// p95 budget per class (end to end), ms.
    pub budget_ms: f64,
}

impl SimilarBench {
    /// Classes whose end-to-end p95 is at or over the budget.
    pub fn over_budget(&self) -> Vec<&SimilarClassLatency> {
        self.classes
            .iter()
            .filter(|c| c.total.p95_ms >= self.budget_ms)
            .collect()
    }
}

/// One query of the mix.
struct Case {
    class: SimilarClass,
    seed: i64,
    opts: SimilarOptions,
}

/// Runs the benchmark in `work` (store, indexes are created inside it).
///
/// # Errors
/// [`Error::Io`], [`Error::Sqlite`], [`Error::Index`], vector store and
/// query errors.
pub fn bench_similar(work: &Path, opts: &SimilarBenchOptions) -> Result<SimilarBench> {
    let t = Instant::now();
    let store = Store::open(work.join("sonar.db"))?;
    let SearchCorpus { libraries, .. } =
        generate_store(&store, &work.join("libraries"), opts.docs, opts.seed)?;
    let (ix, _) = TextIndex::open(work.join("index").join("text"))?;
    ix.rebuild(&store)?;
    let vdir = work.join("index").join("vectors");
    let vector_rows = {
        let (mut vs, _) = VectorStore::open(&vdir)?;
        write_vectors(&mut vs, opts.docs, opts.seed)?
    };
    let setup_secs = t.elapsed().as_secs_f64();
    let vs = VectorStore::open_read(&vdir)?;
    let mix = cases(&vs, opts, &libraries)?;
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(opts.threads)
        .build()
        .map_err(|e| Error::Internal {
            reason: format!("cannot start thread pool: {e}"),
        })?;
    let threads = pool.current_num_threads();
    let runs = pool.install(|| run_mix(&vs, &ix, &mix))?;
    let classes = SimilarClass::ALL
        .iter()
        .map(|&class| {
            let of: Vec<_> = mix
                .iter()
                .zip(&runs)
                .filter(|(c, _)| c.class == class)
                .map(|(_, r)| *r)
                .collect();
            let n = of.len();
            SimilarClassLatency {
                class: class.name(),
                n,
                total: Percentiles::of(of.iter().map(|r| r.0).collect()),
                filter: Percentiles::of(of.iter().map(|r| r.1).collect()),
                scoring: Percentiles::of(of.iter().map(|r| r.2).collect()),
                mean_candidates: of.iter().map(|r| r.3 as f64).sum::<f64>() / n.max(1) as f64,
            }
        })
        .collect();
    Ok(SimilarBench {
        machine: machine(),
        threads,
        docs: opts.docs,
        vector_rows,
        vector_bytes: vs.stats().bytes,
        seed: opts.seed,
        setup_secs,
        limit: SimilarOptions::new().limit,
        classes,
        budget_ms: SEARCH_BUDGET_MS,
    })
}

/// A short untimed warm-up, then every case once: end-to-end, filter and
/// scoring latency (ms) and candidates scored.
fn run_mix(vs: &VectorStore, ix: &TextIndex, mix: &[Case]) -> Result<Vec<(f64, f64, f64, usize)>> {
    for c in mix.iter().take(SimilarClass::ALL.len() * 4) {
        similar(vs, ix, &Seed::File(c.seed), &c.opts)?;
    }
    let mut runs = Vec::with_capacity(mix.len());
    for c in mix {
        let seed = Seed::File(c.seed);
        let t = Instant::now();
        let r = similar(vs, ix, &seed, &c.opts)?;
        let total = ms(t.elapsed());
        let t = Instant::now();
        let allowed = filtered_file_ids(ix, &c.opts.filters)?;
        let filter = ms(t.elapsed());
        let raw = seed_vectors(vs, &seed, None)?;
        let t = Instant::now();
        let again = rank(vs, &raw, Some(c.seed), &allowed, &c.opts)?;
        let scoring = ms(t.elapsed());
        if again != r {
            return Err(Error::Internal {
                reason: "similar and filter + rank disagree".into(),
            });
        }
        runs.push((total, filter, scoring, r.candidates));
    }
    Ok(runs)
}

/// `opts.queries` cases per class, interleaved; seeds are random stored
/// files.
fn cases(vs: &VectorStore, opts: &SimilarBenchOptions, libraries: &[i64]) -> Result<Vec<Case>> {
    if vs.live_rows() == 0 {
        return Err(Error::Internal {
            reason: "similarity benchmark has no vectors".into(),
        });
    }
    let mut r = Rng::new(opts.seed ^ 0x51_3D, 0);
    let mut out = Vec::with_capacity(opts.queries * SimilarClass::ALL.len());
    for q in 0..opts.queries {
        for class in SimilarClass::ALL {
            let seed = loop {
                let id = (r.next() % opts.docs as u64) as i64 + 1;
                if vs.row_of(id).is_some() {
                    break id;
                }
            };
            let mut o = SimilarOptions::new();
            match class {
                SimilarClass::Overall => {}
                SimilarClass::Weighted => o.weights = weights(q),
                SimilarClass::Filtered => o.filters = filter(q, seed, opts.seed, libraries),
                SimilarClass::Unfiltered => o.filters.include_hidden = true,
            }
            out.push(Case {
                class,
                seed,
                opts: o,
            });
        }
    }
    Ok(out)
}

/// Custom weights, rotating over a mix and single aspects.
fn weights(q: usize) -> AspectWeights {
    match q % 4 {
        0 => AspectWeights {
            spectrum: 1.0,
            timbre: 2.0,
            pitch: 0.0,
            amplitude: 0.5,
        },
        1 => AspectWeights::only(Aspect::Timbre),
        2 => AspectWeights::only(Aspect::Pitch),
        _ => AspectWeights {
            spectrum: 0.2,
            timbre: 1.0,
            pitch: 0.7,
            amplitude: 3.0,
        },
    }
}

/// Filters rotating over the seed's category, category + kind, the largest
/// library and a loop BPM range.
fn filter(q: usize, seed_id: i64, corpus_seed: u64, libraries: &[i64]) -> Filters {
    let row = file_row(corpus_seed, (seed_id - 1).max(0) as usize);
    let category = vec![row.category.unwrap_or("kick").to_owned()];
    match q % 4 {
        0 => Filters {
            categories: category,
            ..Filters::default()
        },
        1 => Filters {
            categories: category,
            kinds: vec![row.kind.unwrap_or("one_shot").to_owned()],
            ..Filters::default()
        },
        2 => Filters {
            libraries: libraries.first().copied().into_iter().collect(),
            ..Filters::default()
        },
        _ => Filters {
            kinds: vec!["loop".into()],
            bpm: Some(ValueRange::new(Some(115.0), Some(135.0))),
            ..Filters::default()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Small end-to-end run (the 500k run is `sonar bench similar`).
    #[test]
    fn small_bench_runs_every_class() {
        let tmp = tempfile::tempdir().expect("tmp");
        let opts = SimilarBenchOptions {
            docs: 10_000, // > 2 chunks of 4096: covers the parallel merge
            queries: 4,
            seed: DEFAULT_SEED,
            threads: 2,
        };
        let b = bench_similar(tmp.path(), &opts).expect("bench");
        assert!(b.vector_rows > 9_700 && b.vector_rows <= 10_000);
        assert_eq!(b.classes.len(), SimilarClass::ALL.len());
        for c in &b.classes {
            assert_eq!(c.n, 4, "{}", c.class);
            assert!(c.mean_candidates > 0.0, "{} scored nothing", c.class);
            assert!(c.total.p50_ms <= c.total.p95_ms && c.total.p95_ms <= c.total.max_ms);
        }
        let all = |name| b.classes.iter().find(|c| c.class == name).expect(name);
        assert!(all("filtered").mean_candidates < all("overall").mean_candidates);
        assert!(all("overall").mean_candidates < all("unfiltered").mean_candidates);
    }
}
