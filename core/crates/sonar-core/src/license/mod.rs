//! Licensing (T7.5, ADR-038): a 14-day trial plus offline signed license keys
//! behind a provider-agnostic [`LicenseVerifier`] trait.
//!
//! * [`verify`]: key format, the trait, [`SignedKeyVerifier`] (Ed25519).
//! * [`keys`]: the build's public key, key generation, [`issue`].
//! * [`state`]: `<data dir>/license.json` (trial start, clock guard, key).
//! * [`status`]: [`status()`], [`activate()`], [`deactivate`].
//!
//! Nothing here touches the network or the library database, and none of it
//! panics on any input. What the app does when the license lapses is a UI
//! decision (library data is never touched).

mod date;
mod error;
mod keys;
mod state;
mod status;
#[cfg(test)]
mod tests;
mod verify;

pub use date::Date;
pub use error::LicenseError;
#[cfg(any(test, feature = "dev-keys"))]
pub use keys::dev;
pub use keys::{
    builtin_public_key, generate_keypair, issue, parse_public_key, parse_secret_key, uses_dev_key,
    KeyPair, DEV_PUBLIC_KEY,
};
pub use state::STATE_FILE;
pub use status::{
    activate, activate_with, deactivate, now_unix, status, status_with, LicenseStatus, TRIAL_DAYS,
};
pub use verify::{
    normalize_key, LicenseInfo, LicenseKind, LicenseVerifier, SignedKeyVerifier, KEY_PREFIX,
    PAYLOAD_VERSION, PRODUCT,
};
