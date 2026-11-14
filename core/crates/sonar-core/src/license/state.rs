//! The persisted licensing state: `<data dir>/license.json`.
//!
//! Holds the trial start, the latest time ever seen (clock-rollback guard)
//! and the activated key. It is not user library data and is never touched
//! by analysis code. Honest-user model (ADR-038): deleting the file restarts
//! the trial; that is accepted, this is not DRM.

use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::LicenseError;

/// File name inside the data directory.
pub const STATE_FILE: &str = "license.json";

/// Contents of `license.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct State {
    /// Format version of this file.
    pub v: u32,
    /// Unix seconds of the first status call.
    pub trial_started: i64,
    /// Largest "now" ever observed (unix seconds).
    pub last_seen: i64,
    /// The activated key text (whitespace removed), if any.
    #[serde(default)]
    pub key: Option<String>,
}

/// Result of [`load`].
pub(crate) struct Loaded {
    pub state: State,
    /// No usable file was there: the state is new and not yet written.
    pub fresh: bool,
}

pub(crate) fn path(data_dir: &Path) -> PathBuf {
    data_dir.join(STATE_FILE)
}

impl State {
    fn started_at(t: i64) -> Self {
        Self {
            v: 1,
            trial_started: t,
            last_seen: t,
            key: None,
        }
    }
}

/// Reads the state file, never failing because of its contents.
///
/// Missing file: a new trial starting at `now`. Unreadable or corrupt file
/// (not JSON, wrong shape, truncated): the trial is treated as started at
/// the file's modification time (or `now` if that is unknown), so a damaged
/// file neither errors at the user nor hands out a fresh trial; an activated
/// key stored in it is lost and must be entered again. The caller rewrites
/// the file.
pub(crate) fn load(data_dir: &Path, now: i64) -> Loaded {
    let file = path(data_dir);
    match fs::read(&file) {
        Ok(bytes) => match serde_json::from_slice::<State>(&bytes) {
            Ok(mut state) => {
                state.last_seen = state.last_seen.max(state.trial_started);
                Loaded {
                    state,
                    fresh: false,
                }
            }
            Err(_) => Loaded {
                state: State::started_at(mtime_secs(&file).unwrap_or(now).min(now)),
                fresh: true,
            },
        },
        Err(e) if e.kind() == ErrorKind::NotFound => Loaded {
            state: State::started_at(now),
            fresh: true,
        },
        Err(_) => Loaded {
            state: State::started_at(mtime_secs(&file).unwrap_or(now).min(now)),
            fresh: true,
        },
    }
}

fn mtime_secs(file: &Path) -> Option<i64> {
    let t = fs::metadata(file).ok()?.modified().ok()?;
    let d = t.duration_since(std::time::UNIX_EPOCH).ok()?;
    i64::try_from(d.as_secs()).ok()
}

/// Writes the state atomically: temp file in the same directory, fsync,
/// rename over `license.json`. A crash leaves either the old or the new file.
pub(crate) fn save(data_dir: &Path, state: &State) -> Result<(), LicenseError> {
    fs::create_dir_all(data_dir).map_err(|e| LicenseError::io(data_dir, e))?;
    let json = serde_json::to_vec_pretty(state).map_err(|e| LicenseError::State {
        reason: e.to_string(),
    })?;
    let target = path(data_dir);
    let tmp = data_dir.join(format!("{STATE_FILE}.tmp-{}", std::process::id()));
    let write = || -> std::io::Result<()> {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(&json)?;
        f.sync_all()?;
        fs::rename(&tmp, &target)
    };
    write().map_err(|e| {
        let _ = fs::remove_file(&tmp);
        LicenseError::io(&target, e)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_is_a_fresh_trial() {
        let dir = tempfile::tempdir().expect("tmp");
        let l = load(dir.path(), 1000);
        assert!(l.fresh);
        assert_eq!((l.state.trial_started, l.state.last_seen), (1000, 1000));
    }

    #[test]
    fn save_then_load_round_trips_and_leaves_no_temp_file() {
        let dir = tempfile::tempdir().expect("tmp");
        let s = State {
            key: Some("SONAR1-x.y".into()),
            ..State::started_at(5)
        };
        save(dir.path(), &s).expect("save");
        let l = load(dir.path(), 99);
        assert!(!l.fresh);
        assert_eq!(l.state, s);
        let names: Vec<_> = fs::read_dir(dir.path())
            .expect("ls")
            .map(|e| e.expect("entry").file_name())
            .collect();
        assert_eq!(names, [STATE_FILE]);
    }

    #[test]
    fn save_replaces_an_existing_file_in_place() {
        let dir = tempfile::tempdir().expect("tmp");
        save(dir.path(), &State::started_at(1)).expect("first");
        save(dir.path(), &State::started_at(2)).expect("second");
        assert_eq!(load(dir.path(), 9).state.trial_started, 2);
    }

    #[test]
    fn corrupt_file_counts_from_its_mtime_not_a_fresh_trial() {
        let dir = tempfile::tempdir().expect("tmp");
        fs::write(path(dir.path()), b"{ not json").expect("write");
        let mtime = mtime_secs(&path(dir.path())).expect("mtime");
        let l = load(dir.path(), mtime + 10 * 86_400);
        assert!(l.fresh);
        assert_eq!(l.state.trial_started, mtime);
        assert!(l.state.key.is_none());
    }

    #[test]
    fn inconsistent_last_seen_is_raised_to_trial_start() {
        let dir = tempfile::tempdir().expect("tmp");
        let s = State {
            last_seen: 10,
            ..State::started_at(50)
        };
        save(dir.path(), &s).expect("save");
        assert_eq!(load(dir.path(), 60).state.last_seen, 50);
    }
}
