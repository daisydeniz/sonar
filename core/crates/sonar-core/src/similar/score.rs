//! Brute-force scoring of stored rows against a seed (ADR-024): the
//! library z-score is applied on the fly to each raw row (no normalized
//! copy of the store, no per-row allocation), per-aspect RMS
//! z-differences are combined with the normalized weights, and a bounded
//! max-heap keeps the best `limit` candidates ([`TopK`]; one per parallel
//! work item, merged, ADR-025).

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use serde::Serialize;

use super::AspectWeights;
use crate::dsp::aspects::norm::{STD_FLOOR, Z_CLAMP};
use crate::dsp::aspects::{AspectVectors, NormStats};
use crate::index::vectors::{VectorStore, RAW_ASPECTS};
use crate::{Error, Result};

/// Per-aspect distance of a hit: root mean squared difference of the
/// z-scored vectors (0 = identical; ≈ 1.4 for two independent typical
/// files).
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct AspectDistances {
    /// Spectrum.
    pub spectrum: f32,
    /// Timbre.
    pub timbre: f32,
    /// Pitch.
    pub pitch: f32,
    /// Amplitude.
    pub amplitude: f32,
}

/// z-score of one aspect with `f32` constants, matching
/// [`NormStats::normalize`] (std floored at [`STD_FLOOR`], clamped to
/// ±[`Z_CLAMP`], non-finite → 0).
struct Scaler {
    mean: Vec<f32>,
    inv_sd: Vec<f32>,
}

impl Scaler {
    fn new(stats: &NormStats) -> Self {
        let std = if stats.count() == 0 {
            Vec::new()
        } else {
            stats.std()
        };
        let dim = stats.dim();
        let mean = (0..dim)
            .map(|i| stats.mean().get(i).copied().unwrap_or(0.0) as f32)
            .collect();
        let inv_sd = (0..dim)
            .map(|i| (1.0 / std.get(i).copied().unwrap_or(1.0).max(STD_FLOOR)) as f32)
            .collect();
        Self { mean, inv_sd }
    }

    #[inline]
    fn z(&self, i: usize, x: f32) -> f32 {
        let z = (x - self.mean[i]) * self.inv_sd[i];
        if z.is_finite() {
            z.clamp(-Z_CLAMP, Z_CLAMP)
        } else {
            0.0
        }
    }
}

/// One aspect of the query: its matrix, scaler and the seed's z-scores.
struct Part<'a> {
    weight: f32,
    dim: usize,
    matrix: &'a [f32],
    scaler: Scaler,
    seed_z: Vec<f32>,
}

impl Part<'_> {
    /// Mean squared z-difference between the seed and `row`; `None` if
    /// the row is outside the matrix.
    #[inline]
    fn msd(&self, row: usize) -> Option<f32> {
        let x = self.matrix.get(row * self.dim..(row + 1) * self.dim)?;
        let mut sum = 0.0f32;
        for (i, (&v, &q)) in x.iter().zip(&self.seed_z).enumerate() {
            let d = q - self.scaler.z(i, v);
            sum += d * d;
        }
        Some(sum / self.dim as f32)
    }
}

/// Scores rows of a [`VectorStore`] against one seed.
pub(super) struct Scorer<'a> {
    parts: Vec<Part<'a>>,
}

impl<'a> Scorer<'a> {
    /// Prepares the query: library stats, the seed's z-scores and weights
    /// normalized to sum 1.
    ///
    /// # Errors
    /// [`Error::InvalidQuery`] for invalid weights, [`Error::Dsp`] if the
    /// seed's dimensions do not match the store, [`Error::Internal`] if a
    /// matrix is missing.
    pub(super) fn new(
        vs: &'a VectorStore,
        seed: &AspectVectors,
        weights: &AspectWeights,
    ) -> Result<Self> {
        weights.validate()?;
        let total: f32 = weights.as_array().iter().sum();
        let norm = vs.norm();
        let stats = [&norm.spectrum, &norm.timbre, &norm.pitch, &norm.amplitude];
        let mut parts = Vec::with_capacity(RAW_ASPECTS.len());
        for (((aspect, raw), w), stats) in RAW_ASPECTS
            .into_iter()
            .zip(seed.parts().map(|(_, x)| x))
            .zip(weights.as_array())
            .zip(stats)
        {
            let dim = aspect.dim();
            if raw.len() != dim || stats.dim() != dim {
                return Err(Error::Dsp {
                    reason: format!(
                        "{} seed has {} dimensions, the store {dim}",
                        aspect.name(),
                        raw.len()
                    ),
                });
            }
            let matrix = vs.matrix(aspect).ok_or_else(|| Error::Internal {
                reason: format!("vector store has no {} matrix", aspect.name()),
            })?;
            let scaler = Scaler::new(stats);
            let seed_z = raw
                .iter()
                .enumerate()
                .map(|(i, &v)| scaler.z(i, v))
                .collect();
            parts.push(Part {
                weight: w / total,
                dim,
                matrix,
                scaler,
                seed_z,
            });
        }
        Ok(Self { parts })
    }

    /// Combined distance `Σₐ ŵₐ · √msdₐ` of `row`; `None` if the row is
    /// not in the store.
    pub(super) fn distance(&self, row: usize) -> Option<f32> {
        let mut sum = 0.0f32;
        for p in self.parts.iter().filter(|p| p.weight > 0.0) {
            sum += p.weight * p.msd(row)?.sqrt();
        }
        Some(sum)
    }

    /// Every aspect's distance of `row`, whatever its weight.
    pub(super) fn per_aspect(&self, row: usize) -> AspectDistances {
        let d = |k: usize| {
            self.parts
                .get(k)
                .and_then(|p| p.msd(row))
                .map_or(f32::NAN, f32::sqrt)
        };
        AspectDistances {
            spectrum: d(0),
            timbre: d(1),
            pitch: d(2),
            amplitude: d(3),
        }
    }
}

/// A scored candidate; orders by distance (total order), then file id.
#[derive(Debug, Clone, Copy)]
pub(super) struct Cand {
    pub distance: f32,
    pub file_id: i64,
    pub row: usize,
}

impl PartialEq for Cand {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Cand {}

impl PartialOrd for Cand {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Cand {
    fn cmp(&self, other: &Self) -> Ordering {
        self.distance
            .total_cmp(&other.distance)
            .then(self.file_id.cmp(&other.file_id))
    }
}

/// The `k` smallest candidates seen.
pub(super) struct TopK {
    k: usize,
    heap: BinaryHeap<Cand>,
}

impl TopK {
    pub(super) fn new(k: usize) -> Self {
        Self {
            k,
            heap: BinaryHeap::with_capacity(k.saturating_add(1).min(4096)),
        }
    }

    /// Offers a candidate.
    pub(super) fn push(&mut self, c: Cand) {
        if self.heap.len() < self.k {
            self.heap.push(c);
        } else if self.heap.peek().is_some_and(|worst| c < *worst) {
            self.heap.pop();
            self.heap.push(c);
        }
    }

    /// Offers every candidate of `other` (same `k`): the result keeps the
    /// `k` smallest of both, whatever the merge order (total order).
    pub(super) fn merge(self, other: Self) -> Self {
        let (mut big, small) = if self.heap.len() >= other.heap.len() {
            (self, other)
        } else {
            (other, self)
        };
        for c in small.heap {
            big.push(c);
        }
        big
    }

    /// Best first.
    pub(super) fn into_sorted(self) -> Vec<Cand> {
        self.heap.into_sorted_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn top_k_keeps_the_smallest_in_order_with_id_ties() {
        let mut top = TopK::new(3);
        for (d, id) in [
            (0.5, 1),
            (0.1, 2),
            (0.9, 3),
            (0.1, 0),
            (0.3, 4),
            (f32::NAN, 5),
        ] {
            top.push(Cand {
                distance: d,
                file_id: id,
                row: 0,
            });
        }
        let ids: Vec<i64> = top.into_sorted().iter().map(|c| c.file_id).collect();
        assert_eq!(ids, [0, 2, 4]);
        let mut empty = TopK::new(0);
        empty.push(Cand {
            distance: 0.0,
            file_id: 1,
            row: 0,
        });
        assert!(empty.into_sorted().is_empty());
    }

    /// The parallel scan's per-chunk heaps merge to the sequential result
    /// whatever the split (ties included).
    #[test]
    fn merged_top_k_equals_one_pass_for_any_split() {
        let cands: Vec<Cand> = (0..500i64)
            .map(|id| Cand {
                distance: ((id * 7919) % 97) as f32 / 8.0,
                file_id: id,
                row: id as usize,
            })
            .collect();
        let one_pass = |cs: &[Cand]| {
            let mut t = TopK::new(25);
            for &c in cs {
                t.push(c);
            }
            t
        };
        let ids = |t: TopK| -> Vec<(u32, i64)> {
            t.into_sorted()
                .iter()
                .map(|c| (c.distance.to_bits(), c.file_id))
                .collect()
        };
        let want = ids(one_pass(&cands));
        for split in [1, 3, 64, 499] {
            let merged = cands
                .chunks(split)
                .rev()
                .map(one_pass)
                .fold(TopK::new(25), TopK::merge);
            assert_eq!(ids(merged), want, "split {split}");
        }
    }

    #[test]
    fn scaler_matches_norm_stats() {
        let mut s = NormStats::new(3);
        for x in [[1.0, 5.0, 2.0], [3.0, 5.0, -2.0], [2.0, 5.0, 100.0]] {
            s.push(&x).unwrap();
        }
        let scaler = Scaler::new(&s);
        for x in [[2.5f32, 5.0, 7.0], [-1e9, 5.001, 0.0]] {
            let want = s.normalize(&x);
            for (i, &v) in x.iter().enumerate() {
                assert!((scaler.z(i, v) - want[i]).abs() < 1e-4, "{x:?}[{i}]");
            }
        }
        let empty = Scaler::new(&NormStats::new(2));
        assert_eq!((empty.z(0, 3.0), empty.z(1, 99.0)), (3.0, 8.0));
    }
}
