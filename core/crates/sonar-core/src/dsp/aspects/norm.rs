//! Library-level normalization of aspect vectors (ADR-019).
//!
//! [`NormStats`] keeps a per-dimension running mean and sum of squared
//! deviations with Welford's update (B. P. Welford, "Note on a method for
//! calculating corrected sums of squares and products", Technometrics
//! 4(3), 1962); two accumulators combine with the pairwise formula of
//! T. F. Chan, G. H. Golub & R. J. LeVeque, "Updating formulae and a
//! pairwise algorithm for computing sample variances" (Stanford
//! STAN-CS-79-773, 1979), so rayon workers can `fold` + `reduce`.
//!
//! Normalization is a z-score with the population std floored at
//! [`STD_FLOOR`] (a constant dimension maps to 0 instead of blowing up) and
//! clamped to ±[`Z_CLAMP`]; the output is finite for any input, including
//! stats deserialized from a damaged file.
//!
//! **Overall** = concatenation of the z-scored Spectrum, Timbre, Pitch and
//! Amplitude vectors, each scaled by `wₐ / √dimₐ` ([`OVERALL_WEIGHTS`]):
//! a z-scored aspect has expected squared norm ≈ `dimₐ`, so after scaling
//! each aspect contributes ≈ `wₐ²` to the squared L2 distance regardless
//! of how many dimensions it has.

use serde::{Deserialize, Serialize};

use super::{
    Aspect, AspectVectors, AMPLITUDE_DIM, ASPECTS_VERSION, PITCH_DIM, SPECTRUM_DIM, TIMBRE_DIM,
};
use crate::{Error, Result};

/// Smallest std used as a z-score divisor (raw units).
pub const STD_FLOOR: f64 = 1.0e-3;
/// z-scores are clamped to ±this.
pub const Z_CLAMP: f32 = 8.0;
/// Overall weights for Spectrum, Timbre, Pitch, Amplitude (equal: no
/// aspect is favoured until eval data says otherwise).
pub const OVERALL_WEIGHTS: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
/// Overall vector dimension.
pub const OVERALL_DIM: usize = SPECTRUM_DIM + TIMBRE_DIM + PITCH_DIM + AMPLITUDE_DIM;

/// Running per-dimension mean / variance of fixed-length vectors.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NormStats {
    count: u64,
    mean: Vec<f64>,
    m2: Vec<f64>,
}

impl NormStats {
    /// Empty stats for `dim`-long vectors.
    pub fn new(dim: usize) -> Self {
        Self {
            count: 0,
            mean: vec![0.0; dim],
            m2: vec![0.0; dim],
        }
    }

    /// Vector dimension.
    pub fn dim(&self) -> usize {
        self.mean.len()
    }

    /// Vectors seen.
    pub fn count(&self) -> u64 {
        self.count
    }

    /// Per-dimension mean.
    pub fn mean(&self) -> &[f64] {
        &self.mean
    }

    /// Per-dimension population standard deviation (0 when empty).
    pub fn std(&self) -> Vec<f64> {
        let n = self.count.max(1) as f64;
        self.m2.iter().map(|&m| (m / n).max(0.0).sqrt()).collect()
    }

    /// Adds one vector (non-finite values count as 0). Fails if its length
    /// is not [`Self::dim`].
    pub fn push(&mut self, x: &[f32]) -> Result<()> {
        check_dim(self.dim(), x.len())?;
        self.count += 1;
        let n = self.count as f64;
        for ((mean, m2), &v) in self.mean.iter_mut().zip(&mut self.m2).zip(x) {
            let v = if v.is_finite() { f64::from(v) } else { 0.0 };
            let delta = v - *mean;
            *mean += delta / n;
            *m2 += delta * (v - *mean);
        }
        Ok(())
    }

    /// Folds `other` into `self` (Chan et al.); the result equals pushing
    /// both streams into one accumulator up to rounding.
    pub fn merge(&mut self, other: &NormStats) -> Result<()> {
        check_dim(self.dim(), other.dim())?;
        if other.count == 0 {
            return Ok(());
        }
        let (na, nb) = (self.count as f64, other.count as f64);
        let n = na + nb;
        for (((ma, m2a), &mb), &m2b) in self
            .mean
            .iter_mut()
            .zip(&mut self.m2)
            .zip(&other.mean)
            .zip(&other.m2)
        {
            let delta = mb - *ma;
            *ma += delta * nb / n;
            *m2a += m2b + delta * delta * na * nb / n;
        }
        self.count += other.count;
        Ok(())
    }

    /// z-score of `x`, std floored at [`STD_FLOOR`], clamped to
    /// ±[`Z_CLAMP`]; empty stats and dimensions without stats use mean 0 /
    /// std 1, and any non-finite intermediate becomes 0.
    pub fn normalize(&self, x: &[f32]) -> Vec<f32> {
        let std = if self.count == 0 {
            Vec::new()
        } else {
            self.std()
        };
        x.iter()
            .enumerate()
            .map(|(i, &v)| {
                let mean = self.mean.get(i).copied().unwrap_or(0.0);
                let sd = std.get(i).copied().unwrap_or(1.0);
                let z = ((f64::from(v) - mean) / sd.max(STD_FLOOR)) as f32;
                if z.is_finite() {
                    z.clamp(-Z_CLAMP, Z_CLAMP)
                } else {
                    0.0
                }
            })
            .collect()
    }
}

fn check_dim(expected: usize, got: usize) -> Result<()> {
    if expected == got {
        Ok(())
    } else {
        Err(Error::Dsp {
            reason: format!("aspect vector has {got} dimensions, stats expect {expected}"),
        })
    }
}

/// Normalization stats for every aspect of a library.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LibraryNorm {
    /// [`ASPECTS_VERSION`] the stats were built with.
    pub version: u32,
    /// Spectrum stats.
    pub spectrum: NormStats,
    /// Timbre stats.
    pub timbre: NormStats,
    /// Pitch stats.
    pub pitch: NormStats,
    /// Amplitude stats.
    pub amplitude: NormStats,
}

impl Default for LibraryNorm {
    fn default() -> Self {
        Self::new()
    }
}

impl LibraryNorm {
    /// Empty stats for the current aspect dimensions.
    pub fn new() -> Self {
        Self {
            version: ASPECTS_VERSION,
            spectrum: NormStats::new(SPECTRUM_DIM),
            timbre: NormStats::new(TIMBRE_DIM),
            pitch: NormStats::new(PITCH_DIM),
            amplitude: NormStats::new(AMPLITUDE_DIM),
        }
    }

    /// Files seen.
    pub fn count(&self) -> u64 {
        self.spectrum.count()
    }

    fn parts_mut(&mut self) -> [&mut NormStats; 4] {
        [
            &mut self.spectrum,
            &mut self.timbre,
            &mut self.pitch,
            &mut self.amplitude,
        ]
    }

    fn parts(&self) -> [&NormStats; 4] {
        [&self.spectrum, &self.timbre, &self.pitch, &self.amplitude]
    }

    /// Adds one file's raw vectors; fails (changing nothing) if any
    /// dimension does not match.
    pub fn push(&mut self, v: &AspectVectors) -> Result<()> {
        for (s, (_, x)) in self.parts().iter().zip(v.parts()) {
            check_dim(s.dim(), x.len())?;
        }
        for (s, (_, x)) in self.parts_mut().into_iter().zip(v.parts()) {
            s.push(x)?;
        }
        Ok(())
    }

    /// Folds `other` (e.g. another worker's stats) into `self`; fails
    /// (changing nothing) on a version or dimension mismatch.
    pub fn merge(&mut self, other: &LibraryNorm) -> Result<()> {
        if self.version != other.version {
            return Err(Error::Dsp {
                reason: format!(
                    "aspect stats version {} cannot merge with {}",
                    self.version, other.version
                ),
            });
        }
        for (a, b) in self.parts().iter().zip(other.parts()) {
            check_dim(a.dim(), b.dim())?;
        }
        for (a, b) in self.parts_mut().into_iter().zip(other.parts()) {
            a.merge(b)?;
        }
        Ok(())
    }

    /// z-scored aspects plus Overall (see the module docs).
    pub fn normalize(&self, v: &AspectVectors) -> NormalizedAspects {
        let [spectrum, timbre, pitch, amplitude] = [
            self.spectrum.normalize(&v.spectrum),
            self.timbre.normalize(&v.timbre),
            self.pitch.normalize(&v.pitch),
            self.amplitude.normalize(&v.amplitude),
        ];
        let mut overall = Vec::with_capacity(OVERALL_DIM);
        for (z, w) in [&spectrum, &timbre, &pitch, &amplitude]
            .into_iter()
            .zip(OVERALL_WEIGHTS)
        {
            let scale = w / (z.len().max(1) as f32).sqrt();
            overall.extend(z.iter().map(|&x| x * scale));
        }
        NormalizedAspects {
            spectrum,
            timbre,
            pitch,
            amplitude,
            overall,
        }
    }
}

/// Library-normalized aspect vectors of one file.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct NormalizedAspects {
    /// z-scored Spectrum.
    pub spectrum: Vec<f32>,
    /// z-scored Timbre.
    pub timbre: Vec<f32>,
    /// z-scored Pitch.
    pub pitch: Vec<f32>,
    /// z-scored Amplitude.
    pub amplitude: Vec<f32>,
    /// Weighted concatenation ([`OVERALL_DIM`] values).
    pub overall: Vec<f32>,
}

impl NormalizedAspects {
    /// The vector of `aspect`.
    pub fn get(&self, aspect: Aspect) -> &[f32] {
        match aspect {
            Aspect::Spectrum => &self.spectrum,
            Aspect::Timbre => &self.timbre,
            Aspect::Pitch => &self.pitch,
            Aspect::Amplitude => &self.amplitude,
            Aspect::Overall => &self.overall,
        }
    }
}
