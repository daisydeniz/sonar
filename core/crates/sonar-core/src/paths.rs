//! Where Sonar keeps its data (docs/ARCHITECTURE.md §3).
//!
//! Default: `$HOME/Library/Application Support/Sonar`. Tests and the CLI can
//! point everything elsewhere with `SONAR_HOME`. Resolution is a pure function
//! ([`resolve_data_dir`]) so it can be tested without touching process env.

use std::ffi::OsString;
use std::path::PathBuf;

use crate::{Error, Result};

/// Environment variable that overrides the data directory.
pub const SONAR_HOME_ENV: &str = "SONAR_HOME";

/// File name of the SQLite store inside the data directory.
pub const DB_FILE_NAME: &str = "sonar.db";

/// Data directory relative to `$HOME` when `SONAR_HOME` is not set.
const DEFAULT_SUBDIR: [&str; 3] = ["Library", "Application Support", "Sonar"];

/// Resolves the data directory from the values of `SONAR_HOME` and `HOME`.
///
/// A non-empty `sonar_home` wins. Otherwise the macOS default under `home`
/// is used. Empty values count as unset. The result is always absolute: a
/// relative value is resolved against the current directory (lexically, via
/// [`std::path::absolute`]; symlinks are not resolved), so the store path
/// stays stable if the process later changes directory.
///
/// # Errors
/// [`Error::NoDataDir`] when neither value is set and non-empty;
/// [`Error::Io`] if a relative path cannot be made absolute (current
/// directory unavailable).
pub fn resolve_data_dir(sonar_home: Option<OsString>, home: Option<OsString>) -> Result<PathBuf> {
    let dir = match sonar_home.filter(|v| !v.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => {
            let home = home.filter(|v| !v.is_empty()).ok_or(Error::NoDataDir)?;
            let mut dir = PathBuf::from(home);
            dir.extend(DEFAULT_SUBDIR);
            dir
        }
    };
    std::path::absolute(&dir).map_err(|e| Error::io(dir, e))
}

/// Data directory for this process (reads `SONAR_HOME` and `HOME`).
/// Does not create it.
///
/// # Errors
/// See [`resolve_data_dir`].
pub fn data_dir() -> Result<PathBuf> {
    resolve_data_dir(std::env::var_os(SONAR_HOME_ENV), std::env::var_os("HOME"))
}

/// Path of the store database for this process (`<data dir>/sonar.db`).
///
/// # Errors
/// See [`resolve_data_dir`].
pub fn default_db_path() -> Result<PathBuf> {
    Ok(data_dir()?.join(DB_FILE_NAME))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn os(s: &str) -> Option<OsString> {
        Some(OsString::from(s))
    }

    #[test]
    fn sonar_home_wins_over_home() {
        let dir = resolve_data_dir(os("/tmp/sonar-x"), os("/Users/me"));
        assert_eq!(dir.ok().as_deref(), Some(Path::new("/tmp/sonar-x")));
    }

    #[test]
    fn sonar_home_alone_is_enough() {
        let dir = resolve_data_dir(os("/data/sonar"), None);
        assert_eq!(dir.ok().as_deref(), Some(Path::new("/data/sonar")));
    }

    #[test]
    fn relative_sonar_home_is_made_absolute() {
        let cwd = std::env::current_dir().expect("current dir");
        let dir = resolve_data_dir(os("relative/dir"), os("/Users/me")).expect("resolve");
        assert!(dir.is_absolute(), "{}", dir.display());
        assert_eq!(dir, cwd.join("relative/dir"));
    }

    #[test]
    fn unset_or_empty_sonar_home_falls_back_to_home() {
        let expected = Path::new("/Users/me/Library/Application Support/Sonar");
        for sonar_home in [None, os("")] {
            let dir = resolve_data_dir(sonar_home.clone(), os("/Users/me"));
            assert_eq!(dir.ok().as_deref(), Some(expected), "{sonar_home:?}");
        }
    }

    #[test]
    fn nothing_usable_is_an_error() {
        for (sonar_home, home) in [(None, None), (os(""), os("")), (None, os(""))] {
            let result = resolve_data_dir(sonar_home, home);
            assert!(matches!(result, Err(Error::NoDataDir)), "{result:?}");
        }
    }
}
