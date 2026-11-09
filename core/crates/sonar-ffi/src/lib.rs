//! UniFFI facade over `sonar-core` for the macOS app.
//!
//! This crate is deliberately thin: every exported function delegates to
//! `sonar-core` and contains no product logic (see `docs/ARCHITECTURE.md`).
//! It is built as a static library and packaged by `scripts/build-core.sh`
//! into `app/Generated/SonarCore.xcframework`, with Swift bindings in
//! `app/Generated/SonarCore.swift` (Swift module `SonarCore`, C module
//! `SonarCoreFFI`, configured in `uniffi.toml`).
//!
//! Robustness: the UniFFI scaffolding catches panics so they never unwind
//! into Swift, but a non-throwing export that panics still aborts the app
//! (the bindings call it with `try!`). Every export that can fail must
//! return `Result<_, FfiError>` (a `throws` function in Swift) and must
//! never `unwrap()`.

uniffi::setup_scaffolding!();

/// Version of the Sonar core library (e.g. `"0.1.0"`), shown in the app.
#[uniffi::export]
pub fn core_version() -> String {
    sonar_core::version().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_version_matches_sonar_core() {
        assert_eq!(core_version(), sonar_core::version());
        assert!(!core_version().is_empty());
    }
}
