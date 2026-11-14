//! The one error type of the FFI (ADR-032). Every fallible export returns
//! `Result<_, FfiError>`, which UniFFI turns into a `throws` Swift function
//! and a Swift `FfiError` enum with these cases.

use std::panic::{catch_unwind, AssertUnwindSafe};

use sonar_core::Error;

/// What went wrong, coarse enough for a UI to branch on; `message` is for
/// display and logs only.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum FfiError {
    /// A file, library, collection or other id does not exist (or has no
    /// analysis yet).
    #[error("not found: {message}")]
    NotFound {
        /// Human-readable detail.
        message: String,
    },
    /// The caller passed a value the core cannot use (bad BPM, empty tag,
    /// unknown key spelling, ...). Nothing was changed.
    #[error("invalid input: {message}")]
    InvalidInput {
        /// What is wrong and what is accepted.
        message: String,
    },
    /// A search was superseded by a newer one or by `cancel_search`.
    #[error("cancelled")]
    Cancelled,
    /// A filesystem problem (missing folder, no permission, ...).
    #[error("I/O error: {message}")]
    Io {
        /// Human-readable detail including the path.
        message: String,
    },
    /// The audio file cannot be decoded or is in an unsupported format.
    #[error("audio error: {message}")]
    Audio {
        /// Human-readable detail.
        message: String,
    },
    /// Database, index or internal failure (including a caught panic).
    #[error("internal error: {message}")]
    Internal {
        /// Human-readable detail.
        message: String,
    },
}

impl From<Error> for FfiError {
    fn from(e: Error) -> Self {
        let message = e.to_string();
        match e {
            Error::Cancelled => Self::Cancelled,
            Error::LibraryNotFound { .. }
            | Error::FileNotFound { .. }
            | Error::CollectionNotFound { .. }
            | Error::NoVectors { .. } => Self::NotFound { message },
            Error::InvalidInput { .. } | Error::InvalidQuery { .. } => {
                Self::InvalidInput { message }
            }
            Error::Io { .. } | Error::NonUtf8Path { .. } | Error::NoDataDir => Self::Io { message },
            Error::Decode { .. } | Error::UnsupportedFormat { .. } => Self::Audio { message },
            _ => Self::Internal { message },
        }
    }
}

/// Runs `f`, mapping a core error to [`FfiError`] and a panic to
/// [`FfiError::Internal`], so nothing unwinds into Swift.
pub fn guard<T>(f: impl FnOnce() -> sonar_core::Result<T>) -> Result<T, FfiError> {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(result) => result.map_err(FfiError::from),
        Err(_) => Err(FfiError::Internal {
            message: "panic in the Sonar core".into(),
        }),
    }
}

/// Like [`guard`] for code that already produces an [`FfiError`].
pub fn guard_ffi<T>(f: impl FnOnce() -> Result<T, FfiError>) -> Result<T, FfiError> {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|_| {
        Err(FfiError::Internal {
            message: "panic in the Sonar core".into(),
        })
    })
}

/// Runs infallible `f`; a panic yields `fallback()` instead of unwinding
/// into Swift.
pub fn guard_or<T>(fallback: impl FnOnce() -> T, f: impl FnOnce() -> T) -> T {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|_| fallback())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_errors_map_to_cases() {
        assert!(matches!(
            FfiError::from(Error::Cancelled),
            FfiError::Cancelled
        ));
        assert!(matches!(
            FfiError::from(Error::FileNotFound { id: 3 }),
            FfiError::NotFound { .. }
        ));
        assert!(matches!(
            FfiError::from(Error::InvalidInput { reason: "x".into() }),
            FfiError::InvalidInput { .. }
        ));
        assert!(matches!(
            FfiError::from(Error::Decode {
                path: "a".into(),
                reason: "b".into()
            }),
            FfiError::Audio { .. }
        ));
        assert!(matches!(
            FfiError::from(Error::Embed { reason: "x".into() }),
            FfiError::Internal { .. }
        ));
    }

    #[test]
    fn a_panic_becomes_an_error_value() {
        let r: Result<(), FfiError> = guard(|| panic!("boom"));
        assert!(matches!(r, Err(FfiError::Internal { .. })));
        let r: Result<(), FfiError> = guard_ffi(|| panic!("boom"));
        assert!(matches!(r, Err(FfiError::Internal { .. })));
    }
}
