//! UniFFI facade over `sonar-core` for the macOS app.
//!
//! This crate is deliberately thin: every exported function delegates to
//! `sonar-core` and contains no product logic (see `docs/ARCHITECTURE.md`).
//! It is built as a static library and packaged by `scripts/build-core.sh`
//! into `app/Generated/SonarCore.xcframework`, with Swift bindings in
//! `app/Generated/SonarCore.swift` (Swift module `SonarCore`, C module
//! `SonarCoreFFI`, configured in `uniffi.toml`).
//!
//! Surface (ADR-032): [`SonarEngine`] (an object over
//! `sonar_core::engine::Engine`), the records/enums in [`types`] and
//! [`FfiError`].
//!
//! Robustness: every export that can fail returns `Result<_, FfiError>` (a
//! `throws` function in Swift), runs under [`error::guard`] (a panic becomes
//! `FfiError.Internal`) and never `unwrap()`s. The few non-throwing exports
//! only flip flags or read counters.

mod convert;
mod engine;
mod error;
mod facet_types;
mod similar_types;
mod types;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_similar;

pub use engine::SonarEngine;
pub use error::FfiError;
pub use facet_types::{FacetCount, LibraryFacet, SearchFacets};
pub use similar_types::{
    aspect_weights_from_sliders, AspectWeights, SimilarHit, SimilarPage, SimilarQuery, SimilarSeed,
};
pub use types::*;

uniffi::setup_scaffolding!();

/// Version of the Sonar core library (e.g. `"0.1.0"`), shown in the app.
#[uniffi::export]
pub fn core_version() -> String {
    sonar_core::version().to_owned()
}
