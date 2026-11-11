//! Generations: the set of row files plus `norm.json` in `gen-<n>/`, and
//! the `CURRENT` pointer that names the live one (ADR-023).
//!
//! A generation is replaced as a whole (compaction, reset after a version
//! change): the new directory is written and synced, then `CURRENT` is
//! swapped by rename, then the old directory is deleted. A crash at any
//! point leaves `CURRENT` naming a complete generation; leftovers are
//! removed by the next writer.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use super::file::{as_f32, as_i64, Layout, RowFile, HEADER_LEN};
use super::{RAW_ASPECTS, TOMBSTONE};
use crate::dsp::aspects::{LibraryNorm, ASPECTS_VERSION};
use crate::{Error, Result};

/// Pointer file naming the live generation.
pub(super) const CURRENT_FILE: &str = "CURRENT";
const IDS_FILE: &str = "ids.i64";
const NORM_FILE: &str = "norm.json";
const GEN_PREFIX: &str = "gen-";
/// Rows per rayon task when recomputing stats.
const NORM_CHUNK: usize = 4096;

/// Layout of the id file.
pub(super) const IDS_LAYOUT: Layout = Layout {
    kind: 0,
    dims: 1,
    elem: 8,
};

/// Layout of raw aspect `k` (index into [`RAW_ASPECTS`]).
pub(super) fn aspect_layout(k: usize) -> Layout {
    Layout {
        kind: k as u32 + 1,
        dims: RAW_ASPECTS[k].dim() as u32,
        elem: 4,
    }
}

/// Directory of generation `g`.
pub(super) fn gen_dir(dir: &Path, g: u64) -> PathBuf {
    dir.join(format!("{GEN_PREFIX}{g}"))
}

fn aspect_path(gen: &Path, k: usize) -> PathBuf {
    gen.join(format!("{}.f32", RAW_ASPECTS[k].name()))
}

/// `norm.json`: the stats and how many leading rows they cover.
#[derive(Debug, Serialize, Deserialize)]
struct NormFile {
    rows: u64,
    norm: LibraryNorm,
}

/// An opened generation.
#[derive(Debug)]
pub(super) struct Generation {
    pub dir: PathBuf,
    pub ids: RowFile,
    pub aspects: [RowFile; 4],
    /// Committed rows (same in every file after a writable open).
    pub rows: u64,
    /// Live rows: file id → row (last row wins).
    pub by_file: HashMap<i64, u32>,
    /// Stats over every committed row (live or dead).
    pub norm: LibraryNorm,
}

impl Generation {
    /// Every row file, ids first.
    pub fn files(&self) -> impl Iterator<Item = &RowFile> {
        std::iter::once(&self.ids).chain(&self.aspects)
    }

    /// Mutable access to every row file.
    pub fn files_mut(&mut self) -> impl Iterator<Item = &mut RowFile> {
        std::iter::once(&mut self.ids).chain(&mut self.aspects)
    }

    /// Writes `norm.json` (temp file + rename).
    pub fn save_norm(&self) -> Result<()> {
        let file = NormFile {
            rows: self.rows,
            norm: self.norm.clone(),
        };
        let path = self.dir.join(NORM_FILE);
        let json = serde_json::to_vec(&file).map_err(|e| Error::json(&path, e))?;
        write_atomic(&path, &json, false)
    }
}

/// Reads `CURRENT`; `None` if there is none or it is unreadable.
pub(super) fn read_current(dir: &Path) -> Result<Option<u64>> {
    let path = dir.join(CURRENT_FILE);
    match fs::read_to_string(&path) {
        Ok(s) => Ok(s.trim().parse().ok()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(Error::io(path, e)),
    }
}

/// Points `CURRENT` at generation `g` (synced temp file + rename + dir sync).
pub(super) fn write_current(dir: &Path, g: u64) -> Result<()> {
    write_atomic(&dir.join(CURRENT_FILE), format!("{g}\n").as_bytes(), true)?;
    sync_dir(dir)
}

/// Flushes a directory's entries (new/renamed files) to disk.
pub(super) fn sync_dir(dir: &Path) -> Result<()> {
    File::open(dir)
        .and_then(|d| d.sync_all())
        .map_err(|e| Error::io(dir, e))
}

fn write_atomic(path: &Path, bytes: &[u8], sync: bool) -> Result<()> {
    let tmp = path.with_extension("tmp");
    let mut f = File::create(&tmp).map_err(|e| Error::io(&tmp, e))?;
    f.write_all(bytes).map_err(|e| Error::io(&tmp, e))?;
    if sync {
        f.sync_all().map_err(|e| Error::io(&tmp, e))?;
    }
    fs::rename(&tmp, path).map_err(|e| Error::io(path, e))
}

/// Removes generation directories other than `keep` and temp files that a
/// crashed writer left behind. Touches only names this module creates.
pub(super) fn remove_stale(dir: &Path, keep: Option<u64>) -> Result<()> {
    let entries = fs::read_dir(dir).map_err(|e| Error::io(dir, e))?;
    for entry in entries {
        let entry = entry.map_err(|e| Error::io(dir, e))?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let path = entry.path();
        let stale_gen = name
            .strip_prefix(GEN_PREFIX)
            .and_then(|n| n.parse::<u64>().ok())
            .is_some_and(|g| Some(g) != keep);
        if stale_gen {
            fs::remove_dir_all(&path).map_err(|e| Error::io(&path, e))?;
        } else if name == "CURRENT.tmp" {
            fs::remove_file(&path).map_err(|e| Error::io(&path, e))?;
        }
    }
    Ok(())
}

/// Creates the empty row files of generation `g` (replacing a leftover
/// directory of that number). Does not switch `CURRENT`.
pub(super) fn create(dir: &Path, g: u64) -> Result<PathBuf> {
    let gen = gen_dir(dir, g);
    if gen.exists() {
        fs::remove_dir_all(&gen).map_err(|e| Error::io(&gen, e))?;
    }
    fs::create_dir_all(&gen).map_err(|e| Error::io(&gen, e))?;
    RowFile::create(&gen.join(IDS_FILE), IDS_LAYOUT)?;
    for k in 0..RAW_ASPECTS.len() {
        RowFile::create(&aspect_path(&gen, k), aspect_layout(k))?;
    }
    Ok(gen)
}

/// Opens the generation in `gen`. Committed rows = the smallest of every
/// file's header count and whole rows on disk; a writable open truncates
/// longer files (torn append) and rewrites their headers. Stats come from
/// `norm.json`, extended over rows it does not cover or recomputed if it
/// is missing, damaged or ahead of the rows (persisted when writable).
///
/// # Errors
/// [`Error::Io`]; [`Error::VectorStore`] for a header that does not match
/// this build.
pub(super) fn open(gen: &Path, writable: bool) -> Result<Generation> {
    let (ids, found) = RowFile::open(&gen.join(IDS_FILE), IDS_LAYOUT, writable)?;
    let mut rows = found.header_rows.min(found.len_rows);
    let mut founds = vec![found];
    let mut aspects = Vec::with_capacity(RAW_ASPECTS.len());
    for k in 0..RAW_ASPECTS.len() {
        let (f, found) = RowFile::open(&aspect_path(gen, k), aspect_layout(k), writable)?;
        rows = rows.min(found.header_rows).min(found.len_rows);
        founds.push(found);
        aspects.push(f);
    }
    let aspects: [RowFile; 4] = aspects.try_into().map_err(|_| Error::Internal {
        reason: "vector store aspect count".into(),
    })?;
    // Row numbers are u32 in the id map (4 G files is far beyond any library).
    let rows = rows.min(u64::from(u32::MAX));
    let mut g = Generation {
        dir: gen.into(),
        ids,
        aspects,
        rows,
        by_file: HashMap::new(),
        norm: LibraryNorm::new(),
    };
    if writable {
        for (f, found) in g.files_mut().zip(&founds) {
            let exact = HEADER_LEN + rows * f.layout().row_bytes();
            if found.header_rows != rows || f.mapped_len() != exact {
                tracing::warn!(rows, "vector store: dropping uncommitted rows");
                f.truncate(rows)?;
                f.set_rows(rows)?;
                f.remap()?;
            }
        }
    }
    g.by_file = map_rows(&g.ids, rows)?;
    let (norm, changed) = load_norm(&g)?;
    g.norm = norm;
    if writable && changed {
        g.save_norm()?;
    }
    Ok(g)
}

/// file id → row over the id slots (tombstones skipped, last row wins).
fn map_rows(ids: &RowFile, rows: u64) -> Result<HashMap<i64, u32>> {
    let slots = as_i64(ids.data(rows)).ok_or_else(|| Error::Internal {
        reason: "misaligned vector id map".into(),
    })?;
    let mut map = HashMap::with_capacity(slots.len());
    for (row, &id) in slots.iter().enumerate() {
        if id != TOMBSTONE {
            map.insert(id, row as u32);
        }
    }
    Ok(map)
}

/// Stats for `g` and whether they differ from `norm.json`.
fn load_norm(g: &Generation) -> Result<(LibraryNorm, bool)> {
    let path = g.dir.join(NORM_FILE);
    let saved = fs::read(&path)
        .ok()
        .and_then(|b| serde_json::from_slice::<NormFile>(&b).ok())
        .filter(|f| f.norm.version == ASPECTS_VERSION && f.rows <= g.rows && dims_match(&f.norm));
    match saved {
        Some(f) if f.rows == g.rows => Ok((f.norm, false)),
        Some(f) => {
            let mut norm = f.norm;
            norm.merge(&norm_of_rows(g, f.rows, g.rows)?)?;
            Ok((norm, true))
        }
        None => Ok((norm_of_rows(g, 0, g.rows)?, true)),
    }
}

fn dims_match(n: &LibraryNorm) -> bool {
    let fresh = LibraryNorm::new();
    [
        (&n.spectrum, &fresh.spectrum),
        (&n.timbre, &fresh.timbre),
        (&n.pitch, &fresh.pitch),
        (&n.amplitude, &fresh.amplitude),
    ]
    .iter()
    .all(|(a, b)| a.dim() == b.dim())
}

/// Stats over rows `start..end` of `g` (rayon fold + Chan merge).
pub(super) fn norm_of_rows(g: &Generation, start: u64, end: u64) -> Result<LibraryNorm> {
    let mats: Vec<&[f32]> = g
        .aspects
        .iter()
        .map(|f| as_f32(f.data(end)).unwrap_or(&[]))
        .collect();
    let (start, end) = (start as usize, end as usize);
    (start..end)
        .into_par_iter()
        .with_min_len(NORM_CHUNK)
        .try_fold(LibraryNorm::new, |mut n, r| {
            push_row(&mut n, &mats, r)?;
            Ok(n)
        })
        .try_reduce(LibraryNorm::new, |mut a, b| {
            a.merge(&b)?;
            Ok(a)
        })
}

/// Adds row `r` of the raw matrices `mats` (in [`RAW_ASPECTS`] order).
fn push_row(n: &mut LibraryNorm, mats: &[&[f32]], r: usize) -> Result<()> {
    let parts = [
        &mut n.spectrum,
        &mut n.timbre,
        &mut n.pitch,
        &mut n.amplitude,
    ];
    for ((stats, m), aspect) in parts.into_iter().zip(mats).zip(RAW_ASPECTS) {
        let d = aspect.dim();
        let row = m.get(r * d..(r + 1) * d).ok_or_else(|| Error::Internal {
            reason: "vector row out of range".into(),
        })?;
        stats.push(row)?;
    }
    Ok(())
}
