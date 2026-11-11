//! Sample classifiers (`docs/ARCHITECTURE.md` §2 `classify`). For now the
//! **loop vs one-shot** decision ([`classify_kind`], ADR-020): explainable
//! heuristics on the DSP [`crate::dsp::Features`] of the analysis window —
//! onset-envelope periodicity (Ellis 2007, Scheirer 1998), true duration
//! and the decay tail ([`scores`]) — combined by a fixed logistic
//! ([`kind`]), with an optional, lower-weight filename hint ([`hint`]).
//! Detection never needs the filename (fixture names are neutral and the
//! eval hides names).
//!
//! Pure functions of their inputs: deterministic, no allocation beyond a
//! few per-file buffers, finite outputs for any input (silence, empty,
//! NaN), never panics.

pub mod hint;
pub mod kind;
pub mod scores;

#[cfg(test)]
mod tests;

pub use hint::kind_hint;
pub use kind::{classify_kind, Kind, KindResult, KindSource};
pub use scores::{kind_scores, KindScores};

/// Version of the loop/one-shot definitions (scores, weights, hint words);
/// bump when any changes so stored `kind` values are recomputed.
pub const KIND_VERSION: u32 = 1;
