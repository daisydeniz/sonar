//! One fixed-width row file of the vector store: a 64-byte header followed
//! by `rows × row_bytes` of little-endian data, read through a read-only
//! memory map and written with positional writes (ADR-023).
//!
//! Header layout (all little-endian):
//!
//! | offset | type | field |
//! |---|---|---|
//! | 0 | `[u8; 8]` | magic `SONARVEC` |
//! | 8 | u32 | format version ([`FORMAT_VERSION`]) |
//! | 12 | u32 | kind: 0 = file ids (i64), 1..=4 = raw aspect (f32) |
//! | 16 | u32 | dims (values per row) |
//! | 20 | u32 | bytes per value (8 for ids, 4 for f32) |
//! | 24 | u32 | [`ASPECTS_VERSION`] |
//! | 28 | u32 | reserved (0) |
//! | 32 | u64 | committed rows |
//! | 40 | 24 bytes | zero padding |
//!
//! Data starts at byte 64, so a page-aligned map keeps `f32` and `i64`
//! rows naturally aligned (and 64-byte aligned for SIMD scans).

use std::fs::{File, OpenOptions};
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};

use memmap2::Mmap;

use crate::dsp::aspects::ASPECTS_VERSION;
use crate::{Error, Result};

#[cfg(not(target_endian = "little"))]
compile_error!("the vector store files are little-endian and mapped as native values");

/// Header size in bytes; data rows start here.
pub(super) const HEADER_LEN: u64 = 64;
/// Version of the on-disk layout described in the module docs.
pub const FORMAT_VERSION: u32 = 1;
const MAGIC: [u8; 8] = *b"SONARVEC";

/// What a row file must contain (everything in the header but the count).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Layout {
    /// 0 = ids, 1..=4 = raw aspects in `RAW_ASPECTS` order.
    pub kind: u32,
    /// Values per row.
    pub dims: u32,
    /// Bytes per value.
    pub elem: u32,
}

impl Layout {
    /// Bytes per row.
    pub fn row_bytes(self) -> u64 {
        u64::from(self.dims) * u64::from(self.elem)
    }

    fn encode(self, rows: u64) -> [u8; HEADER_LEN as usize] {
        let mut h = [0u8; HEADER_LEN as usize];
        h[..8].copy_from_slice(&MAGIC);
        let words = [
            FORMAT_VERSION,
            self.kind,
            self.dims,
            self.elem,
            ASPECTS_VERSION,
            0,
        ];
        for (i, w) in words.into_iter().enumerate() {
            h[8 + 4 * i..12 + 4 * i].copy_from_slice(&w.to_le_bytes());
        }
        h[32..40].copy_from_slice(&rows.to_le_bytes());
        h
    }

    /// Committed row count from `h`, or why the header does not match.
    fn check(self, h: &[u8]) -> std::result::Result<u64, String> {
        if h.len() < HEADER_LEN as usize || h[..8] != MAGIC {
            return Err("not a Sonar vector file (bad magic)".into());
        }
        let word = |i: usize| {
            let mut b = [0u8; 4];
            b.copy_from_slice(&h[8 + 4 * i..12 + 4 * i]);
            u32::from_le_bytes(b)
        };
        let checks = [
            ("format version", word(0), FORMAT_VERSION),
            ("kind", word(1), self.kind),
            ("dims", word(2), self.dims),
            ("value size", word(3), self.elem),
            ("aspects version", word(4), ASPECTS_VERSION),
        ];
        for (name, found, want) in checks {
            if found != want {
                return Err(format!("{name} is {found}, this build expects {want}"));
            }
        }
        let mut b = [0u8; 8];
        b.copy_from_slice(&h[32..40]);
        Ok(u64::from_le_bytes(b))
    }
}

/// An open row file.
#[derive(Debug)]
pub(super) struct RowFile {
    path: PathBuf,
    file: File,
    layout: Layout,
    /// Map of the whole file as of the last [`RowFile::remap`].
    map: Mmap,
}

/// What [`RowFile::open`] found on disk.
#[derive(Debug, Clone, Copy)]
pub(super) struct Found {
    /// Row count in the header.
    pub header_rows: u64,
    /// Whole rows the file length holds.
    pub len_rows: u64,
}

impl RowFile {
    /// Creates (or replaces) `path` with an empty file of `layout`.
    pub fn create(path: &Path, layout: Layout) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)
            .map_err(|e| Error::io(path, e))?;
        file.write_all_at(&layout.encode(0), 0)
            .map_err(|e| Error::io(path, e))?;
        let map = map_file(path, &file)?;
        Ok(Self {
            path: path.into(),
            file,
            layout,
            map,
        })
    }

    /// Opens `path` and checks its header against `layout`.
    ///
    /// # Errors
    /// [`Error::Io`], or [`Error::VectorStore`] when the header does not
    /// match (other format/aspects version, dims, kind, not ours).
    pub fn open(path: &Path, layout: Layout, writable: bool) -> Result<(Self, Found)> {
        let file = OpenOptions::new()
            .read(true)
            .write(writable)
            .open(path)
            .map_err(|e| Error::io(path, e))?;
        let len = file.metadata().map_err(|e| Error::io(path, e))?.len();
        let map = map_file(path, &file)?;
        let header_rows = layout.check(&map).map_err(|reason| Error::VectorStore {
            path: path.into(),
            reason,
        })?;
        let len_rows = len.saturating_sub(HEADER_LEN) / layout.row_bytes().max(1);
        let found = Found {
            header_rows,
            len_rows,
        };
        Ok((
            Self {
                path: path.into(),
                file,
                layout,
                map,
            },
            found,
        ))
    }

    /// The layout this file was opened with.
    pub fn layout(&self) -> Layout {
        self.layout
    }

    /// Data bytes of the first `rows` rows (fewer if the map is shorter).
    pub fn data(&self, rows: u64) -> &[u8] {
        let start = HEADER_LEN as usize;
        let want = usize::try_from(rows.saturating_mul(self.layout.row_bytes())).unwrap_or(0);
        let end = start.saturating_add(want).min(self.map.len());
        self.map.get(start..end).unwrap_or(&[])
    }

    /// Writes `bytes` starting at row `row` (append or in-place update).
    pub fn write_rows(&self, row: u64, bytes: &[u8]) -> Result<()> {
        let at = HEADER_LEN + row * self.layout.row_bytes();
        self.file
            .write_all_at(bytes, at)
            .map_err(|e| Error::io(&self.path, e))
    }

    /// Records `rows` as the committed row count in the header.
    pub fn set_rows(&self, rows: u64) -> Result<()> {
        self.file
            .write_all_at(&self.layout.encode(rows), 0)
            .map_err(|e| Error::io(&self.path, e))
    }

    /// Cuts the file to exactly `rows` rows (drops a torn tail).
    pub fn truncate(&self, rows: u64) -> Result<()> {
        self.file
            .set_len(HEADER_LEN + rows * self.layout.row_bytes())
            .map_err(|e| Error::io(&self.path, e))
    }

    /// Flushes data and header to the device.
    pub fn sync(&self) -> Result<()> {
        self.file.sync_all().map_err(|e| Error::io(&self.path, e))
    }

    /// Re-maps the file after it grew.
    pub fn remap(&mut self) -> Result<()> {
        self.map = map_file(&self.path, &self.file)?;
        Ok(())
    }

    /// File size in bytes as currently mapped.
    pub fn mapped_len(&self) -> u64 {
        self.map.len() as u64
    }
}

fn map_file(path: &Path, file: &File) -> Result<Mmap> {
    // SAFETY: the files live in Sonar's private data directory. The only
    // writer is the `VectorStore` holding the directory lock: it appends past
    // the committed rows, overwrites 8-byte id slots in place (tombstones)
    // and only truncates rows past the committed count; compaction writes a
    // new generation and never touches mapped files. Readers only look at
    // committed rows, so they never read bytes that are being cut away.
    unsafe { Mmap::map(file) }.map_err(|e| Error::io(path, e))
}

/// `bytes` as `f32` values (`None` if misaligned or not a whole number).
pub(crate) fn as_f32(bytes: &[u8]) -> Option<&[f32]> {
    let ok = bytes.as_ptr().align_offset(align_of::<f32>()) == 0 && bytes.len().is_multiple_of(4);
    // SAFETY: alignment and length checked above; every bit pattern is a
    // valid `f32`; the result borrows `bytes`.
    ok.then(|| unsafe { std::slice::from_raw_parts(bytes.as_ptr().cast::<f32>(), bytes.len() / 4) })
}

/// `bytes` as `i64` values (`None` if misaligned or not a whole number).
pub(crate) fn as_i64(bytes: &[u8]) -> Option<&[i64]> {
    let ok = bytes.as_ptr().align_offset(align_of::<i64>()) == 0 && bytes.len().is_multiple_of(8);
    // SAFETY: alignment and length checked above; every bit pattern is a
    // valid `i64`; the result borrows `bytes`.
    ok.then(|| unsafe { std::slice::from_raw_parts(bytes.as_ptr().cast::<i64>(), bytes.len() / 8) })
}
