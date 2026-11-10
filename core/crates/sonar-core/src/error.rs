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

    /// A JSON document at `path` (e.g. a fixture manifest) could not be
    /// serialized or parsed.
    #[error("invalid JSON at {}", path.display())]
    Json {
        /// File the JSON was read from or written to.
        path: PathBuf,
        /// Underlying serde_json error (includes line/column when parsing).
        #[source]
        source: serde_json::Error,
    },

    /// No data directory could be determined: neither `SONAR_HOME` nor
    /// `HOME` is set to a non-empty value.
    #[error("cannot determine the Sonar data directory: set SONAR_HOME or HOME")]
    NoDataDir,

    /// A SQLite operation on the store failed.
    ///
    /// The detail is in the message and deliberately not exposed as
    /// `source()`, so `anyhow` chains do not print it twice.
    #[error("database error: {0}")]
    Sqlite(rusqlite::Error),

    /// The database schema version is not one this build can work with
    /// (written by a newer Sonar, or corrupt). The file is left untouched.
    #[error(
        "database schema version {found} is not supported (this build supports up to {supported})"
    )]
    UnsupportedSchema {
        /// `PRAGMA user_version` found in the file.
        found: i64,
        /// Latest schema version this build knows.
        supported: i64,
    },

    /// Applying migration `version` failed; its transaction was rolled back,
    /// so the database stays at `version - 1`.
    #[error("migration {version} ({name}) failed")]
    Migration {
        /// Schema version the migration would have produced.
        version: i64,
        /// Migration name, e.g. `0001_init`.
        name: &'static str,
        /// Underlying SQLite error.
        #[source]
        source: rusqlite::Error,
    },

    /// No library with this id exists in the store.
    #[error("library {id} not found")]
    LibraryNotFound {
        /// Requested `libraries.id`.
        id: i64,
    },

    /// A path that must be stored as text (e.g. a library root) is not
    /// valid UTF-8.
    #[error("path is not valid UTF-8: {}", path.display())]
    NonUtf8Path {
        /// The offending path.
        path: PathBuf,
    },

    /// A required connection pragma did not take effect (e.g. SQLite refused
    /// WAL journaling, or foreign keys could not be enabled).
    #[error("could not set PRAGMA {name} = {expected} (SQLite reports '{found}')")]
    Pragma {
        /// Pragma name, e.g. `journal_mode`.
        name: &'static str,
        /// Value we required.
        expected: &'static str,
        /// Value SQLite reported back.
        found: String,
    },

    /// A background job failed unexpectedly (a panic in the job thread, or
    /// a thread/pool that could not be started). Never carries user data.
    #[error("internal error: {reason}")]
    Internal {
        /// What went wrong.
        reason: String,
    },

    /// The file at `path` could not be decoded as audio: not audio at all,
    /// a damaged header, no decodable frames, or limits exceeded (absurd
    /// channel count / sample rate / packet size).
    #[error("cannot decode {}: {reason}", path.display())]
    Decode {
        /// The audio file.
        path: PathBuf,
        /// Human-readable cause (from the demuxer/decoder or our checks).
        reason: String,
    },

    /// The file at `path` is in a container or codec this build cannot
    /// decode (or is not recognised as audio at all).
    #[error("unsupported audio format in {}: {reason}", path.display())]
    UnsupportedFormat {
        /// The audio file.
        path: PathBuf,
        /// What is unsupported.
        reason: String,
    },
}

impl From<rusqlite::Error> for Error {
    fn from(source: rusqlite::Error) -> Self {
        Self::Sqlite(source)
    }
}

impl Error {
    /// Builds an [`Error::Io`] that records which path failed.
    pub fn io(path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }

    /// Builds an [`Error::Json`] that records which path failed.
    pub fn json(path: impl Into<PathBuf>, source: serde_json::Error) -> Self {
        Self::Json {
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
    fn json_error_names_path_and_keeps_source() {
        let parse = serde_json::from_str::<u32>("not json");
        let Err(source) = parse else {
            panic!("expected a parse error");
        };
        let err = Error::json("/tmp/fixtures.json", source);
        assert_eq!(err.to_string(), "invalid JSON at /tmp/fixtures.json");
        assert!(err.source().is_some());
    }

    #[test]
    fn store_errors_are_descriptive() {
        let err = Error::UnsupportedSchema {
            found: 9,
            supported: 1,
        };
        assert_eq!(
            err.to_string(),
            "database schema version 9 is not supported (this build supports up to 1)"
        );
        let err = Error::from(rusqlite::Error::InvalidQuery);
        assert!(matches!(err, Error::Sqlite(_)));
        let detail = rusqlite::Error::InvalidQuery.to_string();
        assert_eq!(err.to_string(), format!("database error: {detail}"));
        assert!(err.source().is_none(), "detail must not repeat in chains");
        let err = Error::Pragma {
            name: "journal_mode",
            expected: "wal",
            found: "delete".into(),
        };
        assert_eq!(
            err.to_string(),
            "could not set PRAGMA journal_mode = wal (SQLite reports 'delete')"
        );
        assert!(Error::NoDataDir.to_string().contains("SONAR_HOME"));
        assert_eq!(
            Error::LibraryNotFound { id: 7 }.to_string(),
            "library 7 not found"
        );
        let err = Error::NonUtf8Path {
            path: "/x/y".into(),
        };
        assert_eq!(err.to_string(), "path is not valid UTF-8: /x/y");
    }

    #[test]
    fn decode_errors_name_path_and_reason() {
        let err = Error::Decode {
            path: "/s/kick.wav".into(),
            reason: "unexpected end of file".into(),
        };
        assert_eq!(
            err.to_string(),
            "cannot decode /s/kick.wav: unexpected end of file"
        );
        assert!(err.source().is_none());
        let err = Error::UnsupportedFormat {
            path: "/s/a.wav".into(),
            reason: "no audio track".into(),
        };
        assert_eq!(
            err.to_string(),
            "unsupported audio format in /s/a.wav: no audio track"
        );
    }

    #[test]
    fn error_is_send_sync_static() {
        fn assert_bounds<T: Send + Sync + 'static>() {}
        assert_bounds::<Error>();
    }
}
