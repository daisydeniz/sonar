//! Exact duplicates (ADR-036): files with the same bytes.
//!
//! Candidates are files that share `(size, quick_hash)` (the scanner's
//! XXH3-64 of size + first/last 64 KiB, index `files_size_quick_hash`);
//! two files with equal content always share both, so nothing is missed.
//! Each candidate's full content hash — **XXH3-128** over every byte
//! (twox-hash, already a dependency; non-cryptographic, which is fine for
//! finding accidental copies on the user's own disk: 128 bits make a
//! chance collision negligible) — is read from `files.content_hash` or
//! computed and cached there. The scanner clears that column whenever a
//! file's size or mtime changes, and a hash is only written back while
//! the row still has the size and mtime it was computed for. Files whose
//! size on disk differs from the row (changed since the last scan) or that
//! cannot be read (offline, permissions) are skipped and counted, never an
//! error. Zero-byte files are ignored.

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;
use std::sync::atomic::AtomicBool;

use rayon::prelude::*;
use rusqlite::params;
use twox_hash::XxHash3_128;

use super::{is_cancelled, Entry};
use crate::scan::identity::mtime_ns;
use crate::store::retry_busy;
use crate::{Error, Result, Store};

/// Bytes of a cached content hash (XXH3-128, little endian). A cached
/// value of any other length is ignored and recomputed.
pub const CONTENT_HASH_LEN: usize = 16;
/// Read buffer of the hasher; the cancel flag is checked per buffer.
const HASH_BUF: usize = 1 << 20;

/// Outcome of the exact pass.
#[derive(Debug, Default)]
pub(super) struct Exact {
    /// Groups of entry indices (each ≥ 2, ascending file id).
    pub groups: Vec<Vec<usize>>,
    /// Files sharing size and quick hash with another file.
    pub candidates: usize,
    /// Content hashes computed in this run (the rest were cached).
    pub hashed: usize,
    /// Candidates skipped: unreadable, or changed since the last scan.
    pub skipped: usize,
}

/// Finds the exact groups among `entries` (see the module docs).
///
/// # Errors
/// [`Error::Cancelled`]; [`Error::Sqlite`] reading paths.
pub(super) fn find(store: &Store, entries: &[Entry], cancel: Option<&AtomicBool>) -> Result<Exact> {
    let mut by_quick: HashMap<(u64, i64), Vec<usize>> = HashMap::new();
    for (i, e) in entries.iter().enumerate() {
        if let (Some(q), true) = (e.quick_hash, e.size > 0) {
            by_quick.entry((e.size, q)).or_default().push(i);
        }
    }
    let candidates: Vec<usize> = by_quick
        .into_values()
        .filter(|v| v.len() > 1)
        .flatten()
        .collect();
    let mut out = Exact {
        candidates: candidates.len(),
        ..Exact::default()
    };
    // Paths first (one connection, sequential), then hash in parallel.
    let mut todo = Vec::new();
    let mut hashes: Vec<(usize, [u8; CONTENT_HASH_LEN])> = Vec::with_capacity(candidates.len());
    for &i in &candidates {
        match entries[i].content_hash {
            Some(h) => hashes.push((i, h)),
            None => match store.file_path(entries[i].id)? {
                Some(path) => todo.push((i, path)),
                None => out.skipped += 1,
            },
        }
    }
    let computed: Vec<(usize, Option<[u8; CONTENT_HASH_LEN]>)> = todo
        .par_iter()
        .map(|(i, path)| {
            if is_cancelled(cancel) {
                return Err(Error::Cancelled);
            }
            let e = &entries[*i];
            match hash_file(path, e.size, e.mtime, cancel) {
                Ok(h) => Ok((*i, h)),
                Err(err) if err.kind() == io::ErrorKind::Interrupted => Err(Error::Cancelled),
                Err(err) => {
                    tracing::debug!(path = %path.display(), error = %err, "dupes: not hashed");
                    Ok((*i, None))
                }
            }
        })
        .collect::<Result<_>>()?;
    let mut fresh = Vec::new();
    for (i, h) in computed {
        match h {
            Some(h) => {
                fresh.push((i, h));
                hashes.push((i, h));
            }
            None => out.skipped += 1,
        }
    }
    out.hashed = fresh.len();
    if let Err(e) = cache(store, entries, &fresh) {
        // The cache is an optimization; the groups are still right.
        tracing::warn!(error = %e, "dupes: could not cache content hashes");
    }
    let mut by_content: HashMap<(u64, [u8; CONTENT_HASH_LEN]), Vec<usize>> = HashMap::new();
    for (i, h) in hashes {
        by_content.entry((entries[i].size, h)).or_default().push(i);
    }
    out.groups = by_content
        .into_values()
        .filter(|v| v.len() > 1)
        .map(|mut v| {
            v.sort_unstable_by_key(|&i| entries[i].id);
            v
        })
        .collect();
    Ok(out)
}

/// XXH3-128 of the file at `path`, `None` if its size or mtime (ns) no
/// longer match the row's `size` / `mtime` (changed since the last scan).
/// A set cancel flag ends the read with [`io::ErrorKind::Interrupted`].
fn hash_file(
    path: &Path,
    size: u64,
    mtime: i64,
    cancel: Option<&AtomicBool>,
) -> io::Result<Option<[u8; CONTENT_HASH_LEN]>> {
    let mut file = File::open(path)?;
    let meta = file.metadata()?;
    if meta.len() != size || mtime_ns(&meta)? != mtime {
        return Ok(None);
    }
    let mut hasher = XxHash3_128::new();
    let mut buf = vec![0u8; HASH_BUF];
    let mut total = 0u64;
    loop {
        if is_cancelled(cancel) {
            return Err(io::ErrorKind::Interrupted.into());
        }
        let n = match file.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        hasher.write(&buf[..n]);
        total += n as u64;
    }
    Ok((total == size).then(|| hasher.finish_128().to_le_bytes()))
}

/// Writes `hashes` to `files.content_hash` where the row still has the
/// size and mtime the entry was loaded with.
fn cache(
    store: &Store,
    entries: &[Entry],
    hashes: &[(usize, [u8; CONTENT_HASH_LEN])],
) -> Result<()> {
    if hashes.is_empty() {
        return Ok(());
    }
    retry_busy(|| {
        let tx = store.conn().unchecked_transaction()?;
        {
            let mut up = tx.prepare_cached(
                "UPDATE files SET content_hash = ?2 WHERE id = ?1 AND size = ?3 AND mtime = ?4",
            )?;
            for (i, h) in hashes {
                let e = &entries[*i];
                let size = i64::try_from(e.size).unwrap_or(i64::MAX);
                up.execute(params![e.id, &h[..], size, e.mtime])?;
            }
        }
        tx.commit()?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_matches_one_shot_xxh3_128_and_detects_size_changes() {
        let tmp = tempfile::tempdir().expect("tmp");
        let p = tmp.path().join("a.bin");
        let data: Vec<u8> = (0..(3 * HASH_BUF + 17))
            .map(|i| (i * 31 % 251) as u8)
            .collect();
        std::fs::write(&p, &data).expect("write");
        let want = XxHash3_128::oneshot(&data).to_le_bytes();
        let n = data.len() as u64;
        let mtime = mtime_ns(&std::fs::metadata(&p).expect("stat")).expect("mtime");
        assert_eq!(hash_file(&p, n, mtime, None).expect("hash"), Some(want));
        assert_eq!(hash_file(&p, n + 1, mtime, None).expect("hash"), None);
        assert_eq!(hash_file(&p, n, mtime - 1, None).expect("hash"), None);
        assert!(hash_file(&tmp.path().join("gone"), 1, mtime, None).is_err());
        let stop = AtomicBool::new(true);
        let err = hash_file(&p, n, mtime, Some(&stop)).expect_err("cancelled");
        assert_eq!(err.kind(), io::ErrorKind::Interrupted);
    }
}
