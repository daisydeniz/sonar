//! Similarity search (T4.2, ADR-024, docs/ARCHITECTURE.md §5–6): "more
//! like this" for a library file or an external file.
//!
//! **Distance.** Every raw aspect vector (stored row and seed alike) is
//! z-scored with the library stats ([`crate::dsp::aspects::LibraryNorm`],
//! std floored, ±8 clamp). Per aspect `a` of dimension `nₐ`:
//!
//! ```text
//! dₐ(q, x) = √( Σᵢ (z_q,i − z_x,i)² / nₐ )     RMS z-difference (L2 / √nₐ)
//! D(q, x)  = Σₐ wₐ · dₐ / Σₐ wₐ                 reported distance
//! ```
//!
//! Dividing by `nₐ` gives every aspect the same scale whatever its
//! dimension (two unrelated files have `dₐ ≈ √2`), so a weight means the
//! same thing on every slider; `D` is a weighted mean of per-aspect L2
//! distances (a metric). Summing *distances* (ARCHITECTURE §5: Σ wᵢ dᵢ)
//! rather than squared distances (= L2 on the Overall vector) keeps one
//! very different aspect from swamping three matching ones: on the
//! fixtures, same-family top-1 34/34 vs 29/34 (ADR-024). L2 rather than
//! cosine: for one-shots the magnitude of a z-deviation (very long decay,
//! very low pitch) is information, not just its direction (cosine 32/34).
//!
//! **Candidates.** The [`Filters`] of text search (same semantics, hidden
//! files excluded unless asked) select the allowed file ids through the
//! text index ([`crate::search::filtered_file_ids`]); only those with a
//! live vector row are scored, exact brute force over the mmap rows,
//! split into chunks scored in parallel (rayon) and merged (ADR-025: at
//! 500k files p95 ≈ 15 ms end to end, no ANN index). The seed file itself
//! is never a result. Ties order by file id; the result does not depend on
//! the thread count.
//!
//! **Seeds.** A library file uses its stored row ([`Error::NoVectors`] if
//! it has none); an external file is decoded and analyzed on the fly with
//! the `dsp` stage's code path ([`seed::analyze_file`]), then normalized
//! with the library stats. An external copy of a library file therefore
//! finds that file first at distance ≈ 0.

mod score;
pub mod seed;
mod weights;

#[cfg(test)]
mod tests;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rayon::prelude::*;
use serde::Serialize;

pub use score::AspectDistances;
pub use weights::AspectWeights;

use crate::dsp::aspects::AspectVectors;
use crate::index::vectors::VectorStore;
use crate::index::TextIndex;
use crate::search::{filtered_file_ids, Filters, DEFAULT_LIMIT};
use crate::{Error, Result};

/// Allowed ids per parallel work item; each item checks the cancel flag
/// first.
const CANCEL_CHECK_EVERY: usize = 4096;

/// What to find neighbours of.
#[derive(Debug, Clone, PartialEq)]
pub enum Seed {
    /// A library file (`files.id`) with stored vectors.
    File(i64),
    /// An audio file anywhere, analyzed on the fly.
    External(PathBuf),
}

/// How to run a similarity query.
#[derive(Debug, Clone, Default)]
pub struct SimilarOptions {
    /// Maximum number of hits ([`SimilarOptions::new`]: [`DEFAULT_LIMIT`]).
    pub limit: usize,
    /// Which files may be returned (same as text search).
    pub filters: Filters,
    /// Aspect weights.
    pub weights: AspectWeights,
    /// Set to stop the query early ([`Error::Cancelled`]).
    pub cancel: Option<Arc<AtomicBool>>,
}

impl SimilarOptions {
    /// Default weights and filters, [`DEFAULT_LIMIT`] hits.
    pub fn new() -> Self {
        Self {
            limit: DEFAULT_LIMIT,
            ..Self::default()
        }
    }
}

/// One neighbour.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct SimilarHit {
    /// `files.id`.
    pub file_id: i64,
    /// Combined distance (smaller = more similar; see the module docs).
    pub distance: f32,
    /// Distance per aspect, whatever the weights.
    pub aspects: AspectDistances,
}

/// Results of [`similar`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SimilarResults {
    /// The seed's file id (`None` for an external seed).
    pub seed_file_id: Option<i64>,
    /// Allowed files that have vectors and were scored (seed excluded).
    pub candidates: usize,
    /// Nearest first, at most [`SimilarOptions::limit`].
    pub hits: Vec<SimilarHit>,
}

/// Raw vectors of `seed` (stored row or on-the-fly analysis).
///
/// # Errors
/// [`Error::NoVectors`] for a library file without a row; decode errors
/// for an external file (see [`seed::analyze_file`]).
pub fn seed_vectors(
    vs: &VectorStore,
    seed: &Seed,
    cancel: Option<Arc<AtomicBool>>,
) -> Result<AspectVectors> {
    match seed {
        Seed::File(id) => vs.vectors(*id).ok_or(Error::NoVectors { file_id: *id }),
        Seed::External(path) => seed::analyze_file(path, cancel),
    }
}

/// The files most similar to `seed` among those `opts.filters` allow
/// (see the module docs).
///
/// # Errors
/// [`Error::InvalidQuery`] (weights), [`Error::NoVectors`], decode errors
/// of an external seed, [`Error::Index`], [`Error::Cancelled`].
pub fn similar(
    vs: &VectorStore,
    index: &TextIndex,
    seed: &Seed,
    opts: &SimilarOptions,
) -> Result<SimilarResults> {
    opts.weights.validate()?;
    let vectors = seed_vectors(vs, seed, opts.cancel.clone())?;
    let seed_id = match seed {
        Seed::File(id) => Some(*id),
        Seed::External(_) => None,
    };
    let allowed = filtered_file_ids(index, &opts.filters)?;
    rank(vs, &vectors, seed_id, &allowed, opts)
}

/// Scores `allowed` (file ids; ids without a live row are skipped, as is
/// `exclude`) against the raw `seed` vectors and returns the best
/// `opts.limit` (filters in `opts` are not applied again).
///
/// Scoring runs in 4096-id chunks on the caller's rayon pool (ADR-025);
/// call it off the UI thread.
///
/// # Errors
/// [`Error::InvalidQuery`], [`Error::Dsp`] (seed dimensions),
/// [`Error::Cancelled`].
pub fn rank(
    vs: &VectorStore,
    seed: &AspectVectors,
    exclude: Option<i64>,
    allowed: &[i64],
    opts: &SimilarOptions,
) -> Result<SimilarResults> {
    let scorer = score::Scorer::new(vs, seed, &opts.weights)?;
    let cancel = opts.cancel.as_deref();
    let empty = || (score::TopK::new(opts.limit), 0usize);
    // Chunks of the allowed ids are scored in parallel, each worker into
    // its own top-K; the merge keeps the k smallest under the total
    // (distance, file id) order, so the result does not depend on the
    // split.
    let (top, candidates) = allowed
        .par_chunks(CANCEL_CHECK_EVERY)
        .try_fold(empty, |(mut top, mut candidates), chunk| {
            if is_cancelled(cancel) {
                return Err(Error::Cancelled);
            }
            for &file_id in chunk {
                if Some(file_id) == exclude {
                    continue;
                }
                let Some(row) = vs.row_of(file_id).and_then(|r| usize::try_from(r).ok()) else {
                    continue;
                };
                let Some(distance) = scorer.distance(row) else {
                    continue;
                };
                candidates += 1;
                top.push(score::Cand {
                    distance,
                    file_id,
                    row,
                });
            }
            Ok((top, candidates))
        })
        .try_reduce(empty, |(a, n), (b, m)| Ok((a.merge(b), n + m)))?;
    let hits = top
        .into_sorted()
        .into_iter()
        .map(|c| SimilarHit {
            file_id: c.file_id,
            distance: c.distance,
            aspects: scorer.per_aspect(c.row),
        })
        .collect();
    Ok(SimilarResults {
        seed_file_id: exclude,
        candidates,
        hits,
    })
}

fn is_cancelled(flag: Option<&AtomicBool>) -> bool {
    flag.is_some_and(|f| f.load(Ordering::Relaxed))
}
