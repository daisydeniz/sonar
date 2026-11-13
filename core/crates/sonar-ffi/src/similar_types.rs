//! FFI records of the similarity query (ADR-035, T6.6) and their
//! conversions. No product logic: the slider mapping and the validation
//! live in `sonar_core::similar::AspectWeights`.

use sonar_core::similar::{
    AspectWeights as CoreWeights, Seed, SimilarHit as CoreHit, SimilarOptions,
};

use crate::convert;
use crate::engine::MAX_PAGE;
use crate::error::{guard, FfiError};
use crate::types::{FileRow, SearchFilters};

/// What "find similar" starts from.
#[derive(Debug, Clone, uniffi::Enum)]
pub enum SimilarSeed {
    /// A library file; it needs analysis (`FfiError.NotFound` otherwise).
    File {
        /// `FileRow.fileId`.
        file_id: i64,
    },
    /// Any audio file on disk (e.g. dropped on the window); it is decoded
    /// and analyzed on the fly, which takes a moment.
    External {
        /// Absolute path.
        path: String,
    },
}

/// Weight of each aspect in the distance. Only ratios matter. All values
/// must be finite and >= 0 and at least one > 0 (`FfiError.InvalidInput`).
/// Use `aspectWeightsFromSliders` to turn the UI's five sliders into this.
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
pub struct AspectWeights {
    /// Spectral shape (brightness, band energy).
    pub spectrum: f32,
    /// Timbre (MFCC-like texture).
    pub timbre: f32,
    /// Pitch / harmonic content.
    pub pitch: f32,
    /// Amplitude envelope and dynamics.
    pub amplitude: f32,
}

/// One similarity request. Only `seed` is required in Swift:
/// `SimilarQuery(seed: .file(fileId: 7))`.
#[derive(Debug, Clone, uniffi::Record)]
pub struct SimilarQuery {
    /// What to find neighbours of.
    pub seed: SimilarSeed,
    /// `nil` = Overall (all four aspects equal).
    #[uniffi(default = None)]
    pub weights: Option<AspectWeights>,
    /// `nil` = no filters (hidden files still left out). Same record and
    /// semantics as search.
    #[uniffi(default = None)]
    pub filters: Option<SearchFilters>,
    /// Hits wanted; cut to 1000.
    #[uniffi(default = 100)]
    pub limit: u32,
}

/// One neighbour's distances (smaller = more similar; 0 = identical,
/// about 1.4 = unrelated, per aspect and combined).
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
pub struct SimilarHit {
    /// `FileRow.fileId`.
    pub file_id: i64,
    /// Combined distance under the query's weights.
    pub distance: f32,
    /// Spectrum distance, whatever the weights.
    pub spectrum: f32,
    /// Timbre distance.
    pub timbre: f32,
    /// Pitch distance.
    pub pitch: f32,
    /// Amplitude distance.
    pub amplitude: f32,
}

/// Result of `similarTo`: `rows[i]` belongs to `hits[i]`, nearest first.
#[derive(Debug, Clone, uniffi::Record)]
pub struct SimilarPage {
    /// Display rows (hits whose file vanished meanwhile are dropped, from
    /// both lists).
    pub rows: Vec<FileRow>,
    /// Distances, same order and length as `rows`.
    pub hits: Vec<SimilarHit>,
    /// How many files were scored (allowed by the filters and analyzed).
    pub candidates: u64,
}

impl From<&CoreHit> for SimilarHit {
    fn from(h: &CoreHit) -> Self {
        Self {
            file_id: h.file_id,
            distance: h.distance,
            spectrum: h.aspects.spectrum,
            timbre: h.aspects.timbre,
            pitch: h.aspects.pitch,
            amplitude: h.aspects.amplitude,
        }
    }
}

impl From<AspectWeights> for CoreWeights {
    fn from(w: AspectWeights) -> Self {
        Self {
            spectrum: w.spectrum,
            timbre: w.timbre,
            pitch: w.pitch,
            amplitude: w.amplitude,
        }
    }
}

impl From<CoreWeights> for AspectWeights {
    fn from(w: CoreWeights) -> Self {
        Self {
            spectrum: w.spectrum,
            timbre: w.timbre,
            pitch: w.pitch,
            amplitude: w.amplitude,
        }
    }
}

/// Core seed and options of an FFI query; validates the weights.
pub fn query(q: SimilarQuery) -> Result<(Seed, SimilarOptions), FfiError> {
    let seed = match q.seed {
        SimilarSeed::File { file_id } => Seed::File(file_id),
        SimilarSeed::External { path } => Seed::External(path.into()),
    };
    let weights = q.weights.map_or_else(CoreWeights::default, Into::into);
    guard(|| weights.validate())?;
    let opts = SimilarOptions {
        limit: usize::try_from(q.limit)
            .unwrap_or(usize::MAX)
            .min(MAX_PAGE as usize),
        filters: convert::filters(q.filters)?,
        weights,
        cancel: None,
    };
    Ok((seed, opts))
}

/// Weights for the UI's sliders: Overall plus one per aspect, each >= 0
/// (suggested range 0...1). Aspect `a` weighs `overall + slider_a`: Overall
/// is a flat floor under all four, an aspect slider adds emphasis on top.
/// `(1, 0, 0, 0, 0)` is plain Overall; `(0, 0, 0, 1, 0)` is pitch only.
/// Pure and instant. `FfiError.InvalidInput` for negative/non-finite values
/// or all zero.
#[uniffi::export]
pub fn aspect_weights_from_sliders(
    overall: f32,
    spectrum: f32,
    timbre: f32,
    pitch: f32,
    amplitude: f32,
) -> Result<AspectWeights, FfiError> {
    guard(|| CoreWeights::from_sliders(overall, [spectrum, timbre, pitch, amplitude]))
        .map(Into::into)
}
