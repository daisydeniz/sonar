//! Errors of the licensing module. Messages are written to be shown to the
//! user as they are (the app shows `FfiError.License` text verbatim).

use std::io;
use std::path::PathBuf;

use super::Date;

/// What can go wrong when checking, activating or storing a license.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum LicenseError {
    /// The text is not a Sonar license key (wrong prefix, bad encoding,
    /// unparsable payload, absurd length, ...).
    #[error("This is not a valid Sonar license key ({reason}). Please paste the whole key.")]
    Malformed {
        /// What is wrong, for logs and the CLI.
        reason: String,
    },

    /// The key is well-formed but was not signed by Sonar's license key
    /// (altered, truncated payload, or issued for another build).
    #[error("This license key is not valid: its signature does not check out.")]
    BadSignature,

    /// A correctly signed key for a different product.
    #[error("This license key is for another product ('{found}'), not Sonar.")]
    WrongProduct {
        /// The `product` field found in the key.
        found: String,
    },

    /// A key format version this build does not know (a newer Sonar issued it).
    #[error("This license key needs a newer version of Sonar (key format {found}).")]
    UnsupportedVersion {
        /// The `v` field found in the key.
        found: u32,
    },

    /// A subscription key past its last valid day.
    #[error("This license expired on {date}.")]
    Expired {
        /// Last day the key was valid.
        date: Date,
    },

    /// The build's public key (`SONAR_LICENSE_PUBLIC_KEY`) or a secret key
    /// file is not a valid Ed25519 key.
    #[error("Invalid license signing key: {reason}")]
    BadSigningKey {
        /// What is wrong.
        reason: String,
    },

    /// `issue` was asked for something that cannot be a valid key.
    #[error("Cannot issue this license: {reason}")]
    InvalidIssue {
        /// What is wrong.
        reason: String,
    },

    /// Reading or writing the license state file (or a key file) failed.
    #[error("License file problem at {}", path.display())]
    Io {
        /// File or directory concerned.
        path: PathBuf,
        /// Underlying OS error.
        #[source]
        source: io::Error,
    },

    /// The state could not be serialized, or the OS gave no randomness.
    #[error("License state error: {reason}")]
    State {
        /// What went wrong.
        reason: String,
    },
}

impl LicenseError {
    pub(crate) fn io(path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }

    pub(crate) fn malformed(reason: &str) -> Self {
        Self::Malformed {
            reason: reason.to_owned(),
        }
    }

    /// True for problems of the key itself (what a user can fix by pasting
    /// another key), false for disk/system failures.
    pub fn is_key_problem(&self) -> bool {
        !matches!(self, Self::Io { .. } | Self::State { .. })
    }
}
