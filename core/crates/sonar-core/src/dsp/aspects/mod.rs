//! Per-file aspect vectors for similarity (`docs/ARCHITECTURE.md` §5,
//! ADR-019): fixed-length raw `f32` vectors for **Spectrum**, **Timbre**,
//! **Pitch** and **Amplitude**, computed from [`Features`] plus the decoded
//! analysis samples. **Overall** exists only after library normalization
//! ([`norm::LibraryNorm::normalize`]).
//!
//! Every vector has a fixed dimension (the `*_DIM` constants) whatever the
//! input length, every value is finite (silence, NaN and huge inputs
//! included), and the computation is sequential and allocation-order
//! independent, so the same samples give bitwise-identical vectors with a
//! fresh or a reused [`AspectExtractor`].
//!
//! Frame statistics (Spectrum, Timbre, chroma) use only **active** STFT
//! frames — energy within [`ACTIVE_RANGE_DB`] of the loudest frame and
//! above [`ACTIVE_FLOOR`] — so trailing silence or a reverb tail at −90 dB
//! does not dominate a one-shot's description. A file without active
//! frames gets the documented "silent" value of each dimension (mostly 0).
//!
//! Spectrum and Timbre read the log-mel clipped at [`TOP_DB`] below the
//! file's maximum (the `top_db` convention of `librosa.power_to_db`) and
//! MFCCs recomputed from it: the absolute −100 dB floor of
//! [`super::mel::POWER_FLOOR`] would otherwise make a synthetic sound's
//! spectral *shape* depend on its gain wherever its spectrum spans more
//! than 100 dB.

pub mod amplitude;
pub mod norm;
pub mod pitch;
pub mod spectrum;
pub mod timbre;

#[cfg(test)]
mod norm_tests;
#[cfg(test)]
mod tests;

pub use amplitude::AMPLITUDE_DIM;
pub use norm::{LibraryNorm, NormStats, NormalizedAspects, OVERALL_DIM, OVERALL_WEIGHTS};
pub use pitch::PITCH_DIM;
pub use spectrum::SPECTRUM_DIM;
pub use timbre::TIMBRE_DIM;

use super::chroma::ChromaMap;
use super::yin::{Yin, YinConfig};
use super::{Dct, FeatureMatrix, Features, Spectrogram, N_FFT, N_MELS, N_MFCC};
use crate::{Error, Result};

/// Version of the aspect definitions; bump when a dimension's meaning,
/// order or formula changes so stored vectors and stats are recomputed.
pub const ASPECTS_VERSION: u32 = 1;
/// Frames more than this many dB below the loudest frame are inactive.
pub const ACTIVE_RANGE_DB: f32 = 60.0;
/// Frames with total STFT power (Σ|X|², unnormalized) at or below this are
/// inactive (≈ 117 dB below a full-scale sine's frame power).
pub const ACTIVE_FLOOR: f32 = 1.0e-6;
/// Log-mel range (dB below the file maximum) used by Spectrum and Timbre.
pub const TOP_DB: f32 = 80.0;

/// The similarity aspects, in vector order of [`OVERALL_WEIGHTS`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Aspect {
    /// Spectral envelope shape and its descriptors.
    Spectrum,
    /// MFCC timbre statistics (c0 excluded).
    Timbre,
    /// f0 (YIN), voicing and chroma.
    Pitch,
    /// Amplitude envelope and dynamics.
    Amplitude,
    /// Weighted concatenation of the normalized aspects.
    Overall,
}

impl Aspect {
    /// All aspects, Overall last.
    pub const ALL: [Aspect; 5] = [
        Aspect::Spectrum,
        Aspect::Timbre,
        Aspect::Pitch,
        Aspect::Amplitude,
        Aspect::Overall,
    ];

    /// Lower-case name (`spectrum`, …).
    pub fn name(self) -> &'static str {
        match self {
            Aspect::Spectrum => "spectrum",
            Aspect::Timbre => "timbre",
            Aspect::Pitch => "pitch",
            Aspect::Amplitude => "amplitude",
            Aspect::Overall => "overall",
        }
    }

    /// Vector dimension.
    pub fn dim(self) -> usize {
        match self {
            Aspect::Spectrum => SPECTRUM_DIM,
            Aspect::Timbre => TIMBRE_DIM,
            Aspect::Pitch => PITCH_DIM,
            Aspect::Amplitude => AMPLITUDE_DIM,
            Aspect::Overall => OVERALL_DIM,
        }
    }
}

/// Raw (un-normalized) aspect vectors of one file.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AspectVectors {
    /// [`SPECTRUM_DIM`] values, see [`spectrum`].
    pub spectrum: Vec<f32>,
    /// [`TIMBRE_DIM`] values, see [`timbre`].
    pub timbre: Vec<f32>,
    /// [`PITCH_DIM`] values, see [`pitch`].
    pub pitch: Vec<f32>,
    /// [`AMPLITUDE_DIM`] values, see [`amplitude`].
    pub amplitude: Vec<f32>,
}

impl AspectVectors {
    /// The four raw vectors with their aspect, in Overall order.
    pub fn parts(&self) -> [(Aspect, &[f32]); 4] {
        [
            (Aspect::Spectrum, &self.spectrum),
            (Aspect::Timbre, &self.timbre),
            (Aspect::Pitch, &self.pitch),
            (Aspect::Amplitude, &self.amplitude),
        ]
    }
}

/// Reusable aspect extractor for one sample rate (YIN plans, chroma map).
/// Keep one per worker thread, next to its [`super::Analyzer`].
#[derive(Debug)]
pub struct AspectExtractor {
    sample_rate: u32,
    yin: Yin,
    chroma: ChromaMap,
    dct: Dct,
}

impl AspectExtractor {
    /// Extractor for signals at `sample_rate` (normally
    /// [`crate::decode::ANALYSIS_RATE`]).
    pub fn new(sample_rate: u32) -> Result<Self> {
        Ok(Self {
            sample_rate,
            yin: Yin::new(YinConfig::aspects(sample_rate))?,
            chroma: ChromaMap::new(sample_rate, N_FFT),
            dct: Dct::new(N_MELS, N_MFCC)?,
        })
    }

    /// Sample rate this extractor was built for.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Raw aspect vectors of `samples` (mono, at [`Self::sample_rate`])
    /// given their [`Features`] from [`super::Analyzer::analyze`]. Fails
    /// only if the features come from a different sample rate or FFT size.
    pub fn extract(&mut self, samples: &[f32], features: &Features) -> Result<AspectVectors> {
        let spec = &features.spectrogram;
        if spec.sample_rate != self.sample_rate || spec.n_fft != self.chroma.n_fft() {
            return Err(Error::Dsp {
                reason: format!(
                    "aspect extractor is for {} Hz / n_fft {}, features are {} Hz / n_fft {}",
                    self.sample_rate,
                    self.chroma.n_fft(),
                    spec.sample_rate,
                    spec.n_fft
                ),
            });
        }
        let active = active_frames(spec);
        let log_mel = clip_top_db(&features.log_mel);
        let mfcc = self.dct.mfcc(&log_mel);
        let mut v = AspectVectors {
            spectrum: spectrum::compute(&log_mel, spec, &active),
            timbre: timbre::compute(&mfcc, &active),
            pitch: pitch::compute(&mut self.yin, &self.chroma, samples, features, &active)?,
            amplitude: amplitude::compute(samples, self.sample_rate),
        };
        for x in [
            &mut v.spectrum,
            &mut v.timbre,
            &mut v.pitch,
            &mut v.amplitude,
        ] {
            make_finite(x);
        }
        Ok(v)
    }
}

/// Active-frame mask of a spectrogram (see the module docs).
pub fn active_frames(spec: &Spectrogram) -> Vec<bool> {
    let energy: Vec<f32> = spec.frames().map(|f| f.iter().sum()).collect();
    let max = energy.iter().copied().fold(0.0f32, f32::max);
    let gate = (max * 10f32.powf(-ACTIVE_RANGE_DB / 10.0)).max(ACTIVE_FLOOR);
    energy.iter().map(|&e| e.is_finite() && e > gate).collect()
}

/// `m` with every value raised to at least `max(m) − TOP_DB`.
pub fn clip_top_db(m: &FeatureMatrix) -> FeatureMatrix {
    let max = m.data.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let floor = max - TOP_DB;
    let mut out = m.clone();
    for v in &mut out.data {
        *v = v.max(floor);
    }
    out
}

/// Replaces non-finite values with 0 (defensive: every formula is already
/// bounded, this keeps the "always finite" contract unconditional).
fn make_finite(v: &mut [f32]) {
    for x in v {
        if !x.is_finite() {
            *x = 0.0;
        }
    }
}

/// Running mean / population std (f64) of a scalar stream.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct MeanStd {
    n: u64,
    sum: f64,
    sq: f64,
}

impl MeanStd {
    /// Adds one value.
    pub(crate) fn push(&mut self, x: f64) {
        self.n += 1;
        self.sum += x;
        self.sq += x * x;
    }

    /// Mean (0 when empty).
    pub(crate) fn mean(&self) -> f64 {
        if self.n == 0 {
            0.0
        } else {
            self.sum / self.n as f64
        }
    }

    /// Population standard deviation (0 when empty).
    pub(crate) fn std(&self) -> f64 {
        if self.n == 0 {
            return 0.0;
        }
        let m = self.mean();
        (self.sq / self.n as f64 - m * m).max(0.0).sqrt()
    }
}
