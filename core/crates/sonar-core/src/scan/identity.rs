//! File identity: audio extension filter, mtime in ns and the quick hash.
//!
//! `quick_hash` = XXH3-64 (seed 0; Collet, xxHash spec,
//! <https://github.com/Cyan4973/xxHash/blob/dev/doc/xxhash_spec.md>) over
//! `size as u64 LE || head || tail`, where head/tail are the first and last
//! [`QUICK_HASH_CHUNK`] bytes, or the whole file when it is at most
//! `2 × QUICK_HASH_CHUNK` long. It reads ≤ 128 KiB per file, so it is cheap
//! enough for move detection on spinning disks, but it is **not** a content
//! digest: an edit confined to the middle of a large file with unchanged
//! size is invisible to it (the scanner still notices it through mtime).

use std::fs::{File, Metadata};
use std::hash::Hasher;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;
use std::time::UNIX_EPOCH;

use twox_hash::XxHash3_64;

/// File extensions (lowercase, without dot) the scanner indexes. Matching is
/// ASCII case-insensitive.
pub const AUDIO_EXTENSIONS: &[&str] = &["wav", "aif", "aiff", "flac", "mp3", "ogg", "m4a"];

/// Bytes hashed from each end of a file by [`quick_hash`].
pub const QUICK_HASH_CHUNK: u64 = 64 * 1024;

/// True if `ext` (no dot) is one of [`AUDIO_EXTENSIONS`], ignoring ASCII case.
pub fn is_audio_extension(ext: &str) -> bool {
    AUDIO_EXTENSIONS.iter().any(|a| a.eq_ignore_ascii_case(ext))
}

/// True if the file name `name` has an audio extension (`kick.WAV` yes,
/// `kick.wav.asd`, `wav`, `kick.` no).
pub fn is_audio_name(name: &str) -> bool {
    name.rsplit_once('.')
        .is_some_and(|(stem, ext)| !stem.is_empty() && is_audio_extension(ext))
}

/// Quick identity hash of the file at `path`, whose size is `size` (from
/// the same `stat` as the mtime, so a file that shrank in between fails
/// with `UnexpectedEof` instead of hashing inconsistent data).
///
/// Allocates a 128 KiB buffer per call; hot loops should use
/// [`quick_hash_with`] and reuse one.
///
/// # Errors
/// Any I/O error opening, seeking or reading the file.
pub fn quick_hash(path: &Path, size: u64) -> io::Result<u64> {
    quick_hash_with(path, size, &mut Vec::new())
}

/// [`quick_hash`] reading through `buf`, which is grown to
/// `2 × QUICK_HASH_CHUNK` bytes on first use and can be reused across calls
/// (contents are irrelevant).
///
/// # Errors
/// Any I/O error opening, seeking or reading the file.
pub fn quick_hash_with(path: &Path, size: u64, buf: &mut Vec<u8>) -> io::Result<u64> {
    let mut file = File::open(path)?;
    buf.resize(2 * QUICK_HASH_CHUNK as usize, 0);
    let hashed = if size <= 2 * QUICK_HASH_CHUNK {
        // `size` fits in the buffer, so it fits in usize.
        let n = usize::try_from(size).map_err(io::Error::other)?;
        file.read_exact(&mut buf[..n])?;
        n
    } else {
        let chunk = QUICK_HASH_CHUNK as usize;
        file.read_exact(&mut buf[..chunk])?;
        file.seek(SeekFrom::Start(size - QUICK_HASH_CHUNK))?;
        file.read_exact(&mut buf[chunk..])?;
        2 * chunk
    };
    let mut hasher = XxHash3_64::with_seed(0);
    hasher.write(&size.to_le_bytes());
    hasher.write(&buf[..hashed]);
    Ok(hasher.finish())
}

/// Modification time in nanoseconds since the unix epoch (negative before
/// 1970, saturating at the `i64` range).
///
/// # Errors
/// If the platform does not report modification times.
pub(crate) fn mtime_ns(meta: &Metadata) -> io::Result<i64> {
    let clamp = |ns: u128| i64::try_from(ns).unwrap_or(i64::MAX);
    Ok(match meta.modified()?.duration_since(UNIX_EPOCH) {
        Ok(after) => clamp(after.as_nanos()),
        Err(before) => -clamp(before.duration().as_nanos()),
    })
}

/// `u64` hash bit-cast to the `i64` stored in `files.quick_hash`.
pub(crate) fn hash_to_db(hash: u64) -> i64 {
    i64::from_ne_bytes(hash.to_ne_bytes())
}

/// File size as stored in `files.size` (saturating; real files never come
/// close to `i64::MAX`).
pub(crate) fn size_to_db(size: u64) -> i64 {
    i64::try_from(size).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn audio_names() {
        for yes in [
            "a.wav", "B.WAV", "c.Aif", "d.aiff", "e.flac", "f.mp3", "g.ogg", "h.M4A", "x.y.wav",
        ] {
            assert!(is_audio_name(yes), "{yes}");
        }
        for no in [
            "a.wav.asd",
            "wav",
            ".wav",
            "a.",
            "a.txt",
            "a.mp4",
            "a.wave",
            "",
        ] {
            assert!(!is_audio_name(no), "{no}");
        }
    }

    fn hash_bytes(dir: &Path, name: &str, data: &[u8]) -> u64 {
        let p = dir.join(name);
        fs::write(&p, data).expect("write");
        quick_hash(&p, data.len() as u64).expect("hash")
    }

    #[test]
    fn small_files_hash_whole_content() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let a = vec![7_u8; 1000];
        let mut b = a.clone();
        b[500] = 8;
        let ha = hash_bytes(tmp.path(), "a", &a);
        assert_eq!(ha, hash_bytes(tmp.path(), "a2", &a), "deterministic");
        assert_ne!(ha, hash_bytes(tmp.path(), "b", &b));
        // Empty files hash too (just the size prefix).
        let empty = hash_bytes(tmp.path(), "e", b"");
        assert_ne!(empty, ha);
        // Pinned value: changing the recipe invalidates every stored hash
        // (and with it move detection), so it must be a deliberate change.
        let mut h = XxHash3_64::with_seed(0);
        h.write(&0_u64.to_le_bytes());
        assert_eq!(empty, h.finish());
    }

    #[test]
    fn large_files_hash_head_tail_and_size() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let len = 3 * QUICK_HASH_CHUNK as usize;
        let base: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
        let h = hash_bytes(tmp.path(), "base", &base);

        let mut head = base.clone();
        head[10] ^= 1;
        assert_ne!(h, hash_bytes(tmp.path(), "head", &head));
        let mut tail = base.clone();
        tail[len - 10] ^= 1;
        assert_ne!(h, hash_bytes(tmp.path(), "tail", &tail));
        // Documented blind spot: the middle is not read.
        let mut middle = base.clone();
        middle[len / 2] ^= 1;
        assert_eq!(h, hash_bytes(tmp.path(), "middle", &middle));

        // Same head and tail bytes, different size: size is hashed.
        let zeros_a = vec![0_u8; len];
        let zeros_b = vec![0_u8; len + 1];
        assert_ne!(
            hash_bytes(tmp.path(), "za", &zeros_a),
            hash_bytes(tmp.path(), "zb", &zeros_b)
        );
    }

    #[test]
    fn reused_buffer_gives_identical_hashes() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let big: Vec<u8> = (0..300_000_u32).map(|i| (i % 253) as u8).collect();
        let small = b"tiny".to_vec();
        let mut buf = Vec::new();
        // Big first so stale bytes sit in the buffer when the small one runs.
        for (name, data) in [("big", &big), ("small", &small), ("big2", &big)] {
            let p = tmp.path().join(name);
            fs::write(&p, data).expect("write");
            let size = data.len() as u64;
            let with = quick_hash_with(&p, size, &mut buf).expect("hash with");
            assert_eq!(with, quick_hash(&p, size).expect("hash"), "{name}");
        }
        assert_eq!(buf.len(), 2 * QUICK_HASH_CHUNK as usize);
    }

    #[test]
    fn short_or_missing_files_error() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let p = tmp.path().join("short");
        fs::write(&p, [1_u8; 10]).expect("write");
        let err = quick_hash(&p, 11).expect_err("claimed size too large");
        assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof);
        let big = 3 * QUICK_HASH_CHUNK;
        assert!(quick_hash(&p, big).is_err());
        assert!(quick_hash(&tmp.path().join("nope"), 1).is_err());
    }

    #[test]
    fn db_conversions() {
        assert_eq!(hash_to_db(u64::MAX), -1);
        assert_eq!(hash_to_db(5), 5);
        assert_eq!(size_to_db(u64::MAX), i64::MAX);
        let tmp = tempfile::tempdir().expect("tempdir");
        let meta = fs::metadata(tmp.path()).expect("stat");
        assert!(mtime_ns(&meta).expect("mtime") > 1_600_000_000_000_000_000);
    }
}
