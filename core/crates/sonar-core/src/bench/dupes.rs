//! Duplicate finder timing (`sonar bench dupes`, T6.9a, ADR-036; target:
//! near pass ≤ 10 s at 500k files).
//!
//! 1. Untimed setup: the synthetic library of the search benchmark
//!    ([`super::search_corpus`]: real store rows with durations, no audio on
//!    disk) plus one clustered aspect row per present file
//!    ([`super::similar_corpus`]). Every file gets a distinct quick hash;
//!    then, per 200 files, one **planted exact candidate** (a file given the
//!    size and quick hash of its neighbour) and one **planted near pair** (a
//!    file given the duration and vectors of its neighbour).
//! 2. [`find_duplicates`] over the store runs `runs` times with every pass
//!    on; each run's phase timings are reported. The files do not exist,
//!    so the exact pass times the candidate search plus one failed `open`
//!    per candidate (hash I/O is *not* measured; candidates count as
//!    skipped), and the near pass is the full production scan.

use std::path::Path;
use std::time::Instant;

use rusqlite::params;
use serde::Serialize;

use super::search_corpus::{file_row, generate_store};
use super::similar_corpus::write_vectors;
use super::{machine, Machine};
use crate::dupes::{find_duplicates, DupeKind, DupesOptions, DupesTimings};
use crate::index::vectors::VectorStore;
use crate::{Error, Result, Store};

/// Files per planted exact candidate and per planted near pair.
const PLANT_EVERY: usize = 200;

/// What to run.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DupesBenchOptions {
    /// Files in the synthetic library.
    pub docs: usize,
    /// Timed runs.
    pub runs: usize,
    /// Corpus seed.
    pub seed: u64,
    /// Threads of the pool (0 = all cores).
    pub threads: usize,
    /// Near threshold.
    pub threshold: f32,
}

impl Default for DupesBenchOptions {
    fn default() -> Self {
        Self {
            docs: 500_000,
            runs: 3,
            seed: super::search::DEFAULT_SEED,
            threads: 0,
            threshold: crate::dupes::DEFAULT_NEAR_THRESHOLD,
        }
    }
}

/// Result of [`bench_dupes`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DupesBench {
    /// Where it ran.
    pub machine: Machine,
    /// Threads of the pool.
    pub threads: usize,
    /// File rows generated.
    pub docs: usize,
    /// Vector rows stored.
    pub vector_rows: usize,
    /// Planted exact candidates (pairs sharing size + quick hash).
    pub planted_exact: usize,
    /// Planted near pairs.
    pub planted_near: usize,
    /// Untimed setup, s.
    pub setup_secs: f64,
    /// Files considered by the last run.
    pub files: usize,
    /// Exact candidates found by the last run.
    pub exact_candidates: usize,
    /// Files compared in the near pass.
    pub near_files: usize,
    /// Pairs within the duration window.
    pub near_pairs: u64,
    /// Near groups found.
    pub near_groups: usize,
    /// Planted near pairs found as (part of) a near group.
    pub planted_near_found: usize,
    /// Phase timings per run.
    pub runs: Vec<DupesTimings>,
}

/// Runs the benchmark in `work`.
///
/// # Errors
/// [`Error::Io`], [`Error::Sqlite`], vector store and finder errors.
pub fn bench_dupes(work: &Path, opts: &DupesBenchOptions) -> Result<DupesBench> {
    let t = Instant::now();
    let db = work.join("sonar.db");
    let store = Store::open(&db)?;
    generate_store(&store, &work.join("libraries"), opts.docs, opts.seed)?;
    let vdir = work.join("index").join("vectors");
    let (vector_rows, planted_exact, near_pairs) = {
        let (mut vs, _) = VectorStore::open(&vdir)?;
        let rows = write_vectors(&mut vs, opts.docs, opts.seed)?;
        let exact = plant_exact(&store, opts)?;
        let near = plant_near(&store, &mut vs, opts)?;
        (rows, exact, near)
    };
    let setup_secs = t.elapsed().as_secs_f64();
    let vs = VectorStore::open_read(&vdir)?;
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(opts.threads)
        .build()
        .map_err(|e| Error::Internal {
            reason: format!("cannot start thread pool: {e}"),
        })?;
    let find = DupesOptions {
        threshold: opts.threshold,
        ..DupesOptions::default()
    };
    let mut runs = Vec::new();
    let mut last = None;
    for _ in 0..opts.runs.max(1) {
        // A connection is not `Sync`: each run opens its own in the pool
        // (outside the timed phases, as the engine does).
        let r = pool.install(|| find_duplicates(&Store::open(&db)?, Some(&vs), &find))?;
        runs.push(r.timings);
        last = Some(r);
    }
    let r = last.ok_or_else(|| Error::Internal {
        reason: "no run".into(),
    })?;
    let near: Vec<&Vec<i64>> = r
        .groups
        .iter()
        .filter(|g| g.kind == DupeKind::Near)
        .map(|g| &g.file_ids)
        .collect();
    let planted_near_found = near_pairs
        .iter()
        .filter(|(a, b)| near.iter().any(|ids| ids.contains(a) && ids.contains(b)))
        .count();
    Ok(DupesBench {
        machine: machine(),
        threads: pool.current_num_threads(),
        docs: opts.docs,
        vector_rows,
        planted_exact,
        planted_near: near_pairs.len(),
        setup_secs,
        files: r.files,
        exact_candidates: r.exact_candidates,
        near_files: r.near_files,
        near_pairs: r.near_pairs,
        near_groups: near.len(),
        planted_near_found,
        runs,
    })
}

/// File ids (1-based) of plant slot `k`: exact source, near source.
fn slots(k: usize) -> (usize, usize) {
    (k * PLANT_EVERY + 1, k * PLANT_EVERY + PLANT_EVERY / 2)
}

/// Whether corpus file `id` (1-based) is present (not `missing`).
fn present(opts: &DupesBenchOptions, id: usize) -> bool {
    id <= opts.docs && !file_row(opts.seed, id - 1).missing
}

/// Distinct quick hashes, then file `j + 1` copies size and quick hash of
/// `j` per slot. Returns the pairs planted (both present).
fn plant_exact(store: &Store, opts: &DupesBenchOptions) -> Result<usize> {
    let tx = store.conn().unchecked_transaction()?;
    tx.execute("UPDATE files SET quick_hash = id * 7919", [])?;
    let mut n = 0;
    for k in 0..opts.docs / PLANT_EVERY {
        let (j, _) = slots(k);
        if present(opts, j) && present(opts, j + 1) {
            tx.execute(
                "UPDATE files SET (size, quick_hash) = \
                 (SELECT size, quick_hash FROM files WHERE id = ?1) WHERE id = ?2",
                params![j as i64, (j + 1) as i64],
            )?;
            n += 1;
        }
    }
    tx.commit()?;
    Ok(n)
}

/// File `j + 1` gets the duration and vectors of `j` per slot. Returns the
/// planted `(j, j + 1)` id pairs.
fn plant_near(
    store: &Store,
    vs: &mut VectorStore,
    opts: &DupesBenchOptions,
) -> Result<Vec<(i64, i64)>> {
    let tx = store.conn().unchecked_transaction()?;
    let mut pairs = Vec::new();
    let mut rows = Vec::new();
    for k in 0..opts.docs / PLANT_EVERY {
        let (_, j) = slots(k);
        let (a, b) = (j as i64, j as i64 + 1);
        if !(present(opts, j) && present(opts, j + 1)) {
            continue;
        }
        let Some(v) = vs.vectors(a) else { continue };
        tx.execute(
            "UPDATE audio_meta SET duration_ms = \
             (SELECT duration_ms FROM audio_meta WHERE file_id = ?1) WHERE file_id = ?2",
            params![a, b],
        )?;
        rows.push((b, v));
        pairs.push((a, b));
    }
    tx.commit()?;
    let batch: Vec<(i64, &_)> = rows.iter().map(|(id, v)| (*id, v)).collect();
    vs.append(&batch)?;
    Ok(pairs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_run_finds_the_planted_near_pairs() {
        let tmp = tempfile::tempdir().expect("tmp");
        let opts = DupesBenchOptions {
            docs: 3_000,
            runs: 1,
            ..DupesBenchOptions::default()
        };
        let b = bench_dupes(tmp.path(), &opts).expect("bench");
        assert!(b.planted_near > 5 && b.planted_exact > 5);
        assert_eq!(b.planted_near_found, b.planted_near);
        assert!(b.exact_candidates >= 2 * b.planted_exact);
        assert_eq!(b.runs.len(), 1);
        assert!(b.near_files > 2_500 && b.near_pairs > 0);
    }
}
