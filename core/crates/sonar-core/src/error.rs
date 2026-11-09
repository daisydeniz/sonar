//! Error type shared by every `sonar-core` module.
//!
//! Library code returns [`Result`]; the CLI wraps it in `anyhow`, the FFI
//! facade maps each variant to an error enum (never a panic).

use std::io;
use std::path::PathBuf;

/// Convenience alias for results produced by `sonar-core`.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors produced by `sonar-core`.
///
/// Marked `#[non_exhaustive]` so new variants can land with new features
/// without breaking downstream matches (CLI, FFI facade).
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// A filesystem operation on `path` failed (unreadable, missing, ...).
    #[error("I/O error at {}", path.display())]
    Io {
        /// File or directory the operation targeted.
        path: PathBuf,
        /// Underlying OS error.
        #[source]
        source: io::Error,
    },

    /// The operation was stopped through its cancellation flag.
    #[error("operation cancelled")]
    Cancelled,
}

impl Error {
    /// Builds an [`Error::Io`] that records which path failed.
    pub fn io(path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error as _;

    #[test]
    fn io_error_names_path_and_keeps_source() {
        let err = Error::io(
            "/samples/kick.wav",
            io::Error::new(io::ErrorKind::NotFound, "gone"),
        );
        assert_eq!(err.to_string(), "I/O error at /samples/kick.wav");
        let source = err.source().map(ToString::to_string);
        assert_eq!(source.as_deref(), Some("gone"));
    }

    #[test]
    fn io_error_from_real_filesystem_failure() {
        // Parent directory never exists, so the path cannot exist either.
        let path = std::env::temp_dir()
            .join(format!("sonar-core-missing-{}", std::process::id()))
            .join("x.wav");
        let result: Result<Vec<u8>> = std::fs::read(&path).map_err(|e| Error::io(&path, e));
        match result {
            Err(Error::Io { path: p, source }) => {
                assert_eq!(p, path);
                assert_eq!(source.kind(), io::ErrorKind::NotFound);
            }
            other => panic!("expected Error::Io, got {other:?}"),
        }
    }

    #[test]
    fn cancelled_has_no_source() {
        assert!(Error::Cancelled.source().is_none());
        assert_eq!(Error::Cancelled.to_string(), "operation cancelled");
    }

    #[test]
    fn error_is_send_sync_static() {
        fn assert_bounds<T: Send + Sync + 'static>() {}
        assert_bounds::<Error>();
    }
}
