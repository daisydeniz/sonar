//! Sonar core library.
//!
//! All product logic lives here (see `docs/ARCHITECTURE.md`). The macOS app
//! talks to it through the UniFFI facade crate; the `sonar` CLI exposes every
//! feature headless so it can be tested and benchmarked without UI.

pub mod bench;
pub mod classify;
pub mod decode;
pub mod dsp;
pub mod error;
pub mod eval;
pub mod fixtures;
pub mod index;
pub mod jobs;
pub mod key;
pub mod meta;
pub mod paths;
pub mod quiet_panic;
pub mod scan;
pub mod search;
pub mod similar;
pub mod store;
pub mod tempo;
pub mod text;
pub mod watch;

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
