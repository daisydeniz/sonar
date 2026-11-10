//! Disk and index helpers of `sonar bench watch`: the copy writer and the
//! index totals used to verify it.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use super::corpus::{self, rel_path};
use crate::{Error, Result, Store};

/// Slowest accepted copy rate in files per second.
pub const MIN_COPY_RATE: f64 = 0.01;

/// Rejects copy rates that are not finite or below [`MIN_COPY_RATE`].
///
/// # Errors
/// [`Error::Internal`] describing the valid range.
pub(crate) fn validate_rate(rate: Option<f64>) -> Result<()> {
    match rate {
        Some(r) if !r.is_finite() || r < MIN_COPY_RATE => Err(Error::Internal {
            reason: format!("copy rate must be a finite number >= {MIN_COPY_RATE} files/s"),
        }),
        _ => Ok(()),
    }
}

/// `(files not missing, summed size)` of library `library_id` in the index
/// at `db`.
pub(crate) fn index_totals(db: &Path, library_id: i64) -> Result<(u64, u64)> {
    let store = Store::open(db)?;
    let (n, bytes): (i64, i64) = store.conn().query_row(
        "SELECT COUNT(*), COALESCE(SUM(size), 0) FROM files \
         WHERE library_id = ?1 AND status <> 'missing'",
        [library_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    Ok((
        u64::try_from(n).unwrap_or(0),
        u64::try_from(bytes).unwrap_or(0),
    ))
}

/// Creates files `first..first + count` of the corpus layout (new folders
/// beyond the base library), written in 1 KB chunks, paced to `rate_hz`.
/// Returns when the last file is closed, and the bytes written.
pub(crate) fn write_files(
    root: &Path,
    first: usize,
    count: usize,
    rate_hz: Option<f64>,
) -> Result<(Instant, u64)> {
    validate_rate(rate_hz)?;
    let templates = corpus::templates();
    let start = Instant::now();
    let mut bytes = 0;
    for i in 0..count {
        if let Some(hz) = rate_hz {
            let due = Duration::try_from_secs_f64(i as f64 / hz).map_err(|e| Error::Internal {
                reason: format!("copy pacing: {e}"),
            })?;
            if let Some(wait) = (start + due).checked_duration_since(Instant::now()) {
                thread::sleep(wait);
            }
        }
        let path = root.join(rel_path(first + i));
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;
        }
        let data = corpus::content(&templates, first + i);
        let mut f = fs::File::create(&path).map_err(|e| Error::io(&path, e))?;
        for chunk in data.chunks(1024) {
            f.write_all(chunk).map_err(|e| Error::io(&path, e))?;
            thread::yield_now();
        }
        bytes += data.len() as u64;
    }
    Ok((Instant::now(), bytes))
}
