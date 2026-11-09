//! Sonar core library.
//!
//! All product logic lives here (see `docs/ARCHITECTURE.md`). The macOS app
//! talks to it through the UniFFI facade crate; the `sonar` CLI exposes every
//! feature headless so it can be tested and benchmarked without UI.

pub mod decode;
pub mod error;
pub mod fixtures;
pub mod paths;
pub mod scan;
pub mod store;

pub use error::{Error, Result};
pub use store::{Library, Store};

/// Version of the core library, shown in the app and CLI.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_semver_like() {
        assert_eq!(version().split('.').count(), 3);
    }
}
