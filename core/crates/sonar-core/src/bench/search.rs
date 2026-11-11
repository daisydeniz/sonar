//! Search latency benchmark (`sonar bench search`; budget: p95 < 50 ms at
//! 500k files, docs/PRODUCT.md).
//!
//! 1. Writes a synthetic library of `docs` files into a fresh store
//!    ([`super::search_corpus`], untimed).
//! 2. Builds the on-disk text index from the store with the production
//!    path ([`TextIndex::open`] + [`TextIndex::rebuild`]), timed, and
//!    measures its size.
//! 3. Drops it and opens it again as the app does at start-up; times the
//!    open and the first query separately (page cache warm from the build,
//!    so "cold-ish": no OS-cache-cold numbers without `sudo purge`).
//! 4. Runs the query mix ([`super::search_queries`]) once untimed (warm-up),
//!    then `rounds` times, interleaving the classes, and reports latency
//!    percentiles per [`QueryClass`].

use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use serde::Serialize;

use super::search_corpus::{generate_store, SearchCorpus};
use super::search_queries::{cases, Case, QueryClass};
use super::{machine, Machine};
use crate::index::{OpenState, TextIndex};
use crate::search::{search, Filters, SearchOptions, Synonyms};
use crate::{Error, Result, Store};

/// Latency budget per query class (p95), in milliseconds.
pub const SEARCH_BUDGET_MS: f64 = 50.0;

/// Seed of the synthetic corpus (fixed: runs are comparable).
pub const DEFAULT_SEED: u64 = 0x5EED_50AA;

/// What to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchBenchOptions {
    /// Files in the synthetic library.
    pub docs: usize,
    /// Timed passes over the query mix.
    pub rounds: usize,
    /// Corpus seed.
    pub seed: u64,
}

impl Default for SearchBenchOptions {
    fn default() -> Self {
        Self {
            docs: 500_000,
            rounds: 10,
            seed: DEFAULT_SEED,
        }
    }
}

/// Latency of one query class.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ClassLatency {
    /// Class name (`as_you_type`, ...).
    pub class: &'static str,
    /// Timed queries.
    pub n: usize,
    /// Median, ms.
    pub p50_ms: f64,
    /// 95th percentile (nearest rank), ms.
    pub p95_ms: f64,
    /// 99th percentile (nearest rank), ms.
    pub p99_ms: f64,
    /// Slowest, ms.
    pub max_ms: f64,
    /// Mean number of matching files (shows the queries hit real work).
    pub mean_matches: f64,
    /// The query of this class with the highest median latency.
    pub slowest_query: String,
    /// Its median latency, ms.
    pub slowest_query_p50_ms: f64,
}

/// The first query after opening the index.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FirstQuery {
    /// Query text (with facets).
    pub query: String,
    /// Latency, ms.
    pub ms: f64,
    /// Matching files.
    pub matches: usize,
}

/// Result of [`bench_search`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SearchBench {
    /// Where it ran.
    pub machine: Machine,
    /// File rows generated.
    pub docs: usize,
    /// Documents in the index (generated minus `missing` files).
    pub indexed_docs: u64,
    /// Libraries the files are spread over.
    pub libraries: usize,
    /// Corpus seed.
    pub seed: u64,
    /// Writing the store rows (untimed setup), s.
    pub generate_secs: f64,
    /// Index build from the store (read rows, index, commit, merge), s.
    pub build_secs: f64,
    /// Index size on disk, bytes.
    pub index_bytes: u64,
    /// Index segments after the build.
    pub segments: usize,
    /// Re-opening the built index (as at app start), ms.
    pub open_ms: f64,
    /// First query after the re-open.
    pub first_query: FirstQuery,
    /// Timed passes over the mix.
    pub rounds: usize,
    /// Per-class latency, in [`QueryClass::ALL`] order.
    pub classes: Vec<ClassLatency>,
    /// p95 budget per class, ms.
    pub budget_ms: f64,
}

impl SearchBench {
    /// Classes whose p95 is at or over the budget.
    pub fn over_budget(&self) -> Vec<&ClassLatency> {
        self.classes
            .iter()
            .filter(|c| c.p95_ms >= self.budget_ms)
            .collect()
    }
}

/// Runs the benchmark in `work` (store, library folders and index are
/// created inside it).
///
/// # Errors
/// [`Error::Io`], [`Error::Sqlite`], [`Error::Index`]; an index that does
/// not open as ready after its build is reported as [`Error::Internal`].
pub fn bench_search(work: &Path, opts: &SearchBenchOptions) -> Result<SearchBench> {
    let t = Instant::now();
    let store = Store::open(work.join("sonar.db"))?;
    let SearchCorpus { libraries, .. } =
        generate_store(&store, &work.join("libraries"), opts.docs, opts.seed)?;
    let generate_secs = t.elapsed().as_secs_f64();

    let dir = work.join("index").join("text");
    let (ix, _) = TextIndex::open(&dir)?;
    let report = ix.rebuild(&store)?;
    drop(ix);
    let index_bytes = dir_bytes(&dir)?;

    let t = Instant::now();
    let (ix, state) = TextIndex::open(&dir)?;
    let open_ms = ms(t.elapsed());
    if state != OpenState::Ready {
        return Err(Error::Internal {
            reason: format!("benchmark index did not reopen as ready: {state:?}"),
        });
    }
    let synonyms = Synonyms::builtin();
    let first = "kick";
    let first_opts = SearchOptions {
        facets: true,
        ..SearchOptions::default()
    };
    let t = Instant::now();
    let r = search(&ix, &synonyms, first, &first_opts)?;
    let first_query = FirstQuery {
        query: first.to_owned(),
        ms: ms(t.elapsed()),
        matches: r.total,
    };

    let mix = cases(&libraries);
    run_mix(&ix, &synonyms, &mix)?;
    // Per case: latency of every round, and its match count.
    let mut runs: Vec<(Vec<f64>, usize)> = vec![(Vec::new(), 0); mix.len()];
    for _ in 0..opts.rounds {
        for (run, (elapsed, total)) in runs.iter_mut().zip(run_mix(&ix, &synonyms, &mix)?) {
            run.0.push(ms(elapsed));
            run.1 = total;
        }
    }
    let classes = QueryClass::ALL
        .iter()
        .map(|&class| {
            let of_class: Vec<(&Case, &(Vec<f64>, usize))> = mix
                .iter()
                .zip(&runs)
                .filter(|(c, _)| c.class == class)
                .collect();
            latency(class.name(), &of_class)
        })
        .collect();

    Ok(SearchBench {
        machine: machine(),
        docs: opts.docs,
        indexed_docs: report.docs,
        libraries: libraries.len(),
        seed: opts.seed,
        generate_secs,
        build_secs: report.elapsed.as_secs_f64(),
        index_bytes,
        segments: ix.searcher().segment_readers().len(),
        open_ms,
        first_query,
        rounds: opts.rounds,
        classes,
        budget_ms: SEARCH_BUDGET_MS,
    })
}

/// Runs every case once; latency and match count per case.
fn run_mix(ix: &TextIndex, synonyms: &Synonyms, mix: &[Case]) -> Result<Vec<(Duration, usize)>> {
    mix.iter()
        .map(|c| {
            let t = Instant::now();
            let r = search(ix, synonyms, &c.query, &c.opts)?;
            Ok((t.elapsed(), r.total))
        })
        .collect()
}

pub(crate) fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1e3
}

/// Percentiles over every run of `runs` (the cases of one class, each
/// with its per-round latencies and match count).
fn latency(class: &'static str, runs: &[(&Case, &(Vec<f64>, usize))]) -> ClassLatency {
    let sorted = |t: &mut Vec<f64>| t.sort_by(f64::total_cmp);
    let mut times: Vec<f64> = runs.iter().flat_map(|(_, (t, _))| t.clone()).collect();
    sorted(&mut times);
    let n = times.len();
    let matches: usize = runs.iter().map(|(_, (t, m))| m * t.len()).sum();
    let (slowest_query, slowest_query_p50_ms) = runs
        .iter()
        .map(|(c, (t, _))| {
            let mut t = t.clone();
            sorted(&mut t);
            (describe(c), percentile(&t, 0.50))
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap_or_default();
    ClassLatency {
        class,
        n,
        p50_ms: percentile(&times, 0.50),
        p95_ms: percentile(&times, 0.95),
        p99_ms: percentile(&times, 0.99),
        max_ms: times.last().copied().unwrap_or(0.0),
        mean_matches: if n == 0 {
            0.0
        } else {
            matches as f64 / n as f64
        },
        slowest_query,
        slowest_query_p50_ms,
    }
}

/// `"kick 1" sort relevance`, `"" sort bpm desc + filters`.
fn describe(c: &Case) -> String {
    let mut s = format!("{:?} sort {}", c.query, c.opts.sort.field.name());
    if c.opts.sort.descending {
        s.push_str(" desc");
    }
    if c.opts.filters != Filters::default() {
        s.push_str(" + filters");
    }
    s
}

/// Nearest-rank percentile of sorted values (0 when empty).
pub(crate) fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = (p * sorted.len() as f64).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

/// Total size of the regular files directly in `dir` (a Tantivy index is
/// flat).
fn dir_bytes(dir: &Path) -> Result<u64> {
    let mut total = 0;
    for entry in fs::read_dir(dir).map_err(|e| Error::io(dir, e))? {
        let entry = entry.map_err(|e| Error::io(dir, e))?;
        let meta = entry.metadata().map_err(|e| Error::io(entry.path(), e))?;
        if meta.is_file() {
            total += meta.len();
        }
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_use_nearest_rank() {
        let v: Vec<f64> = (1..=100).map(f64::from).collect();
        assert_eq!(percentile(&v, 0.50), 50.0);
        assert_eq!(percentile(&v, 0.95), 95.0);
        assert_eq!(percentile(&v, 0.99), 99.0);
        assert_eq!(percentile(&[3.0], 0.95), 3.0);
        assert_eq!(percentile(&[], 0.5), 0.0);
    }

    /// Small end-to-end run (the 500k run is `sonar bench search`).
    #[test]
    fn small_bench_runs_every_class_on_disk() {
        let tmp = tempfile::tempdir().expect("tmp");
        let opts = SearchBenchOptions {
            docs: 2_000,
            rounds: 1,
            seed: DEFAULT_SEED,
        };
        let b = bench_search(tmp.path(), &opts).expect("bench");
        assert!(
            b.indexed_docs > 1_900 && b.indexed_docs <= 2_000,
            "{}",
            b.indexed_docs
        );
        assert!(b.index_bytes > 0 && b.segments >= 1);
        assert!(b.first_query.matches > 0);
        assert_eq!(b.classes.len(), QueryClass::ALL.len());
        for c in &b.classes {
            assert!(c.n > 0, "{}", c.class);
            assert!(c.mean_matches > 0.0, "{} matches nothing", c.class);
            assert!(c.slowest_query.contains(" sort "), "{}", c.slowest_query);
            assert!(c.p50_ms <= c.p95_ms && c.p95_ms <= c.p99_ms && c.p99_ms <= c.max_ms);
        }
    }
}
