//! Near-duplicate distance between two stored vector rows (ADR-036).
//!
//! Rows are z-scored on the fly with the library stats (the
//! [`Scaler`] of similarity search, ADR-024). Per aspect `a` the distance is
//! the RMS z-difference `dₐ = √(Σᵢ δᵢ² / nₐ)`, as in similarity search,
//! except that Amplitude leaves out three dimensions ([`AMPLITUDE_SKIP`]):
//! its absolute level ([`LOUDNESS`]; a gain change is the same sound) and
//! the two that depend on where the file starts ([`ATTACK`] and the first
//! envelope bin: the envelope smoothing is truncated at sample 0, so a hit
//! that starts on the first sample and the same hit after a few ms of
//! decoder delay differ by z ≈ 1.8 in attack time). The pair distance is
//! the **largest** per-aspect distance, so a near duplicate must match on
//! every aspect; [`Metric::within`] abandons a pair as soon as one aspect's
//! partial sum is over the threshold.
//!
//! **Prefilter.** [`Metric::pre_row`] keeps [`PRE_DIMS`] z-scores per file
//! (four per aspect) in a small dense matrix; the partial sum over an
//! aspect's four dims is a lower bound of its full sum, so a pair whose
//! partial sum is over that aspect's budget ([`pre_pass`]) can be dropped
//! without changing any result.

use crate::dsp::aspects::amplitude::{ATTACK, CREST, DECAY, EFFECTIVE, ENV, LOUDNESS};
use crate::dsp::aspects::pitch::{F0, PC_COS, PC_SIN, VOICED};
use crate::dsp::aspects::spectrum::{CENTROID, FLATNESS, ROLLOFF_IDX};
use crate::dsp::aspects::Aspect;
use crate::index::vectors::{VectorStore, RAW_ASPECTS};
use crate::similar::Scaler;
use crate::{Error, Result};

/// Amplitude dimensions the duplicate metric leaves out (see the module
/// docs).
pub const AMPLITUDE_SKIP: [usize; 3] = [ENV, ATTACK, LOUDNESS];

/// Distances per aspect, in [`RAW_ASPECTS`] order.
pub type AspectDists = [f32; 4];

/// One aspect: its raw matrix, z-scaler and the dimensions left out.
struct Part<'a> {
    matrix: &'a [f32],
    dim: usize,
    scaler: Scaler,
    /// Bit `i` set: dimension `i` is left out (every aspect has < 64).
    skip: u64,
    /// Dimensions that count.
    used: f32,
}

impl Part<'_> {
    /// Sum of squared z-differences of rows `a` and `b`, stopping early
    /// once it passes `limit`; `None` if a row is outside the matrix.
    #[inline]
    fn sq(&self, a: usize, b: usize, limit: f32) -> Option<f32> {
        let x = self.matrix.get(a * self.dim..(a + 1) * self.dim)?;
        let y = self.matrix.get(b * self.dim..(b + 1) * self.dim)?;
        let mut sum = 0.0f32;
        for (i, (&u, &v)) in x.iter().zip(y).enumerate() {
            if self.skip >> i & 1 == 1 {
                continue;
            }
            let d = self.scaler.z(i, u) - self.scaler.z(i, v);
            sum += d * d;
            // Checking every 8 dims keeps the loop cheap.
            if i % 8 == 7 && sum > limit {
                return Some(sum);
            }
        }
        Some(sum)
    }
}

/// Pair distance over a [`VectorStore`].
pub(crate) struct Metric<'a> {
    parts: Vec<Part<'a>>,
}

impl<'a> Metric<'a> {
    /// Prepares the library stats of `vs`.
    ///
    /// # Errors
    /// [`Error::Internal`] if a matrix is missing.
    pub(crate) fn new(vs: &'a VectorStore) -> Result<Self> {
        let norm = vs.norm();
        let stats = [&norm.spectrum, &norm.timbre, &norm.pitch, &norm.amplitude];
        let mut parts = Vec::with_capacity(RAW_ASPECTS.len());
        for (aspect, stats) in RAW_ASPECTS.into_iter().zip(stats) {
            let matrix = vs.matrix(aspect).ok_or_else(|| Error::Internal {
                reason: format!("vector store has no {} matrix", aspect.name()),
            })?;
            let dim = aspect.dim();
            let skip = if aspect == Aspect::Amplitude {
                AMPLITUDE_SKIP.iter().fold(0u64, |m, &i| m | 1 << i)
            } else {
                0
            };
            parts.push(Part {
                matrix,
                dim,
                scaler: Scaler::new(stats),
                skip,
                used: (dim - skip.count_ones() as usize) as f32,
            });
        }
        Ok(Self { parts })
    }

    /// Every aspect's distance between rows `a` and `b`; `None` if a row
    /// is not in the store.
    pub(crate) fn per_aspect(&self, a: usize, b: usize) -> Option<AspectDists> {
        let mut out = [0.0; 4];
        for (o, p) in out.iter_mut().zip(&self.parts) {
            *o = (p.sq(a, b, f32::INFINITY)? / p.used).sqrt();
        }
        Some(out)
    }

    /// The pair distance (largest aspect distance) of rows `a` and `b` if
    /// it is at most `threshold`, else `None` (also for unknown rows).
    /// Aspects are tried in order of how often they reject a non-duplicate
    /// first; every aspect stops at its own budget.
    pub(crate) fn within(&self, a: usize, b: usize, threshold: f32) -> Option<f32> {
        let t2 = threshold * threshold;
        let mut worst = 0.0f32;
        for k in CHECK_ORDER {
            let p = &self.parts[k];
            let budget = t2 * p.used;
            let sq = p.sq(a, b, budget)?;
            if sq > budget {
                return None;
            }
            worst = worst.max(sq / p.used);
        }
        Some(worst.sqrt())
    }

    /// The prefilter z-scores of `row` (0 outside the matrix).
    pub(crate) fn pre_row(&self, row: usize) -> PreRow {
        let mut out = [0.0; PRE_DIMS];
        let dims = PRE
            .iter()
            .flat_map(|(k, dims)| dims.iter().map(move |&i| (*k, i)));
        for (o, (k, i)) in out.iter_mut().zip(dims) {
            *o = self.z(k, row, i);
        }
        out
    }

    /// Per-aspect budgets of [`pre_pass`] for `threshold` (`t² · nₐ`).
    pub(crate) fn pre_budgets(&self, threshold: f32) -> [f32; 4] {
        PRE.map(|(k, _)| threshold * threshold * self.parts[k].used)
    }

    /// z-score of dimension `i` of `aspect` (index into [`RAW_ASPECTS`])
    /// in row `row`; 0 outside the matrix.
    pub(crate) fn z(&self, aspect: usize, row: usize, i: usize) -> f32 {
        let p = &self.parts[aspect];
        p.matrix
            .get(row * p.dim + i)
            .map_or(0.0, |&x| p.scaler.z(i, x))
    }
}

/// Order in which [`Metric::within`] checks the aspects: Pitch (18 dims)
/// and Amplitude (19 used) are short and separate most unrelated sounds of
/// similar length, then Spectrum (38) and Timbre (57).
const CHECK_ORDER: [usize; 4] = [2, 3, 0, 1];

/// z-scores per file kept by the prefilter.
pub(crate) const PRE_DIMS: usize = 16;

/// Prefilter dims: four per aspect, in [`CHECK_ORDER`] — broad descriptors
/// that vary a lot across a library (f0 and pitch class; decay, effective
/// duration, crest, a mid envelope bin; centroid, roll-off, flatness, a low
/// band; the first four MFCC means). Never a dimension the metric skips.
const PRE: [(usize, [usize; 4]); 4] = [
    (2, [VOICED, F0, PC_COS, PC_SIN]),
    (3, [ENV + 4, DECAY, EFFECTIVE, CREST]),
    (0, [CENTROID, ROLLOFF_IDX, FLATNESS, 2]),
    (1, [0, 1, 2, 3]),
];

/// Prefilter z-scores of one file.
pub(crate) type PreRow = [f32; PRE_DIMS];

/// Whether `a` and `b` may be within the threshold whose per-aspect
/// budgets are `budgets` ([`Metric::pre_budgets`]).
#[inline]
pub(crate) fn pre_pass(a: &PreRow, b: &PreRow, budgets: &[f32; 4]) -> bool {
    for (k, budget) in budgets.iter().enumerate() {
        let mut sum = 0.0f32;
        for i in 4 * k..4 * k + 4 {
            let d = a[i] - b[i];
            sum += d * d;
        }
        if sum > *budget {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::aspects::AspectVectors;

    fn row(seed: f32) -> AspectVectors {
        let v = |a: Aspect, off: f32| -> Vec<f32> {
            (0..a.dim())
                .map(|i| ((i as f32 + off) * 0.7 + seed).sin())
                .collect()
        };
        AspectVectors {
            spectrum: v(Aspect::Spectrum, 0.0),
            timbre: v(Aspect::Timbre, 1.0),
            pitch: v(Aspect::Pitch, 2.0),
            amplitude: v(Aspect::Amplitude, 3.0),
        }
    }

    #[test]
    fn within_matches_per_aspect_and_ignores_level_and_start() {
        let tmp = tempfile::tempdir().expect("tmp");
        let (mut vs, _) = VectorStore::open(tmp.path()).expect("store");
        let (a, b) = (row(0.0), row(0.4));
        let mut louder = a.clone();
        louder.amplitude[LOUDNESS] += 30.0;
        louder.amplitude[ATTACK] -= 1.0;
        louder.amplitude[ENV] += 0.3;
        let others: Vec<AspectVectors> = (0..20).map(|i| row(i as f32)).collect();
        let mut batch = vec![(1, &a), (2, &b), (3, &louder)];
        batch.extend(others.iter().enumerate().map(|(i, v)| (10 + i as i64, v)));
        vs.append(&batch).expect("append");
        let m = Metric::new(&vs).expect("metric");
        let d = m.per_aspect(0, 1).expect("rows");
        let max = d.iter().fold(0.0f32, |x, &y| x.max(y));
        assert!(max > 0.0);
        assert_eq!(m.within(0, 1, max * 1.001), Some(max));
        assert_eq!(m.within(0, 1, max * 0.999), None);
        assert_eq!(m.per_aspect(0, 2), Some([0.0; 4]), "level ignored");
        assert_eq!(m.within(0, 2, 0.0), Some(0.0));
        assert_eq!(m.within(0, 999, 10.0), None);
        assert!((m.z(3, 0, 1) - m.z(3, 2, 1)).abs() < 1e-6);
        // The prefilter never drops a pair the full metric keeps.
        for t in [0.05, 0.2, 0.5, 1.0] {
            let budgets = m.pre_budgets(t);
            for x in 0..23 {
                for y in 0..23 {
                    if m.within(x, y, t).is_some() {
                        assert!(pre_pass(&m.pre_row(x), &m.pre_row(y), &budgets));
                    }
                }
            }
        }
        assert!(!pre_pass(&m.pre_row(0), &m.pre_row(5), &m.pre_budgets(0.0)));
    }

    #[test]
    fn prefilter_dims_are_counted_and_distinct() {
        for (k, dims) in PRE {
            let aspect = RAW_ASPECTS[k];
            for (n, &i) in dims.iter().enumerate() {
                assert!(i < aspect.dim());
                assert!(!dims[..n].contains(&i));
                assert!(aspect != Aspect::Amplitude || !AMPLITUDE_SKIP.contains(&i));
            }
        }
        let mut order: Vec<usize> = PRE.iter().map(|(k, _)| *k).collect();
        assert_eq!(order, CHECK_ORDER);
        order.sort_unstable();
        assert_eq!(order, [0, 1, 2, 3]);
    }
}
