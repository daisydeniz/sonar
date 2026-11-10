//! Generates a realistic sample-library tree of many small files, and small
//! edits to it, for the scan benchmark.
//!
//! Layout: `Pack NNNN/<group>/<kind>/<name>.<ext>`; 64 files per leaf
//! folder, 8 leaf folders per pack, so 50,000 files make ~100 packs and
//! ~800 leaf folders, three to four levels deep. Every file is a valid
//! 16 kHz mono 16-bit WAV of 1-4 KB with unique content (the file index is
//! written into the first samples), carrying one of the scanner's audio
//! extensions (the scanner does not look inside, so the non-wav extensions
//! just exercise the extension filter). Each pack also has a `readme.txt`
//! that the scanner must skip.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use rayon::prelude::*;

use crate::fixtures::wav::{encode, SampleFormat, WavSpec};
use crate::{Error, Result};

/// Files per leaf folder.
const PER_LEAF: usize = 64;
/// Leaf folders per pack.
const LEAVES_PER_PACK: usize = 8;
/// Frames of the WAV templates (1-4 KB of 16-bit mono).
const TEMPLATE_FRAMES: [usize; 5] = [500, 900, 1300, 1700, 2100];
/// Byte offset of the first sample in the templates (canonical 44-byte header).
const DATA_OFFSET: usize = 44;

const LEAVES: [(&str, &str); LEAVES_PER_PACK] = [
    ("Drums", "Kicks"),
    ("Drums", "Snares"),
    ("Drums", "Hats & Cymbals"),
    ("Loops", "Bass"),
    ("Loops", "Synth Melodic"),
    ("FX", "Risers"),
    ("Vocals", "Phrases"),
    ("One Shots", "Misc"),
];
const EXTENSIONS: [&str; 20] = [
    "wav", "wav", "wav", "wav", "wav", "wav", "wav", "wav", "wav", "wav", "wav", "wav", "wav",
    "wav", "aif", "aiff", "flac", "mp3", "ogg", "m4a",
];
const KEYS: [&str; 6] = ["Am", "Dm", "F#m", "C", "Gmaj", "Ebm"];

/// What was written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Corpus {
    /// Audio files written.
    pub files: usize,
    /// Directories created (packs and leaves, not the root).
    pub dirs: usize,
    /// Total bytes of audio files.
    pub bytes: u64,
}

/// Relative path of audio file number `index`; deterministic.
pub fn rel_path(index: usize) -> PathBuf {
    let leaf = index / PER_LEAF;
    let (group, kind) = LEAVES[leaf % LEAVES_PER_PACK];
    let ext = EXTENSIONS[index % EXTENSIONS.len()];
    let bpm = 80 + (index * 7) % 90;
    let name = format!(
        "{}_{index:06}_{}_{bpm}bpm.{ext}",
        kind.split(' ').next().unwrap_or("Sample"),
        KEYS[index % KEYS.len()]
    );
    [
        format!("Pack {:04}", leaf / LEAVES_PER_PACK),
        group.to_owned(),
        kind.to_owned(),
        name,
    ]
    .iter()
    .collect()
}

pub(crate) fn templates() -> Vec<Vec<u8>> {
    TEMPLATE_FRAMES
        .iter()
        .map(|&n| {
            let tone: Vec<f32> = (0..n).map(|i| 0.4 * (i as f32 * 0.05).sin()).collect();
            encode(&WavSpec::new(16_000, 1, SampleFormat::Int16), &tone).bytes
        })
        .collect()
}

/// File `index` as bytes: a template with the index in the first samples.
pub(crate) fn content(templates: &[Vec<u8>], index: usize) -> Vec<u8> {
    let mut b = templates[index % templates.len()].clone();
    // Cannot be out of range: the shortest template has 1,000 data bytes.
    b[DATA_OFFSET..DATA_OFFSET + 4].copy_from_slice(&(index as u32).to_le_bytes());
    b
}

fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    fs::write(path, bytes).map_err(|e| Error::io(path, e))
}

/// Writes `files` audio files under `root` (created if missing), in
/// parallel, one task per leaf folder.
///
/// # Errors
/// [`Error::Io`] on the first file or folder that cannot be written.
pub fn generate_tree(root: &Path, files: usize) -> Result<Corpus> {
    fs::create_dir_all(root).map_err(|e| Error::io(root, e))?;
    let templates = templates();
    let leaves = files.div_ceil(PER_LEAF);
    let results: Vec<Result<u64>> = (0..leaves)
        .into_par_iter()
        .map(|leaf| {
            let first = leaf * PER_LEAF;
            let last = (first + PER_LEAF).min(files);
            let rel = rel_path(first);
            let dir = root.join(rel.parent().unwrap_or(Path::new("")));
            fs::create_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
            if leaf % LEAVES_PER_PACK == 0 {
                if let Some(pack) = dir.parent().and_then(Path::parent) {
                    write(&pack.join("readme.txt"), b"Sample pack. Not audio.\n")?;
                }
            }
            let mut bytes = 0;
            for i in first..last {
                let data = content(&templates, i);
                write(&root.join(rel_path(i)), &data)?;
                bytes += data.len() as u64;
            }
            Ok(bytes)
        })
        .collect();
    let mut bytes = 0;
    for r in results {
        bytes += r?;
    }
    Ok(Corpus {
        files,
        dirs: leaves + leaves.div_ceil(LEAVES_PER_PACK),
        bytes,
    })
}

/// A small batch of edits applied by [`mutate`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mutations {
    /// Files whose content changed (one byte appended).
    pub changed: usize,
    /// Files renamed within their folder.
    pub renamed: usize,
    /// Files deleted.
    pub deleted: usize,
    /// New files added in a new folder.
    pub added: usize,
}

/// Edits about 1 % of a corpus of `files`: appends a byte to every 100th
/// file (offset 3), renames every 100th (offset 50), deletes every 200th
/// (offset 77), and adds `files / 200` new files in `Added`.
///
/// # Errors
/// [`Error::Io`].
pub fn mutate(root: &Path, files: usize) -> Result<Mutations> {
    let mut m = Mutations {
        changed: 0,
        renamed: 0,
        deleted: 0,
        added: 0,
    };
    for i in (3..files).step_by(100) {
        let p = root.join(rel_path(i));
        let mut f = fs::OpenOptions::new()
            .append(true)
            .open(&p)
            .map_err(|e| Error::io(&p, e))?;
        f.write_all(&[0]).map_err(|e| Error::io(&p, e))?;
        m.changed += 1;
    }
    for i in (50..files).step_by(100) {
        let from = root.join(rel_path(i));
        let mut to = from.clone().into_os_string();
        to.push(".moved.wav");
        fs::rename(&from, &to).map_err(|e| Error::io(&from, e))?;
        m.renamed += 1;
    }
    for i in (77..files).step_by(200) {
        let p = root.join(rel_path(i));
        fs::remove_file(&p).map_err(|e| Error::io(&p, e))?;
        m.deleted += 1;
    }
    let added_dir = root.join("Added");
    fs::create_dir_all(&added_dir).map_err(|e| Error::io(&added_dir, e))?;
    let templates = templates();
    for k in 0..files / 200 {
        let data = content(&templates, files + k);
        write(&added_dir.join(format!("new_{k:05}.wav")), &data)?;
        m.added += 1;
    }
    Ok(m)
}

/// Recursively lists regular files under `root` whose names have an audio
/// extension, sorted. Unreadable folders are skipped.
pub fn list_audio(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        for entry in rd.flatten() {
            let path = entry.path();
            match entry.file_type() {
                Ok(t) if t.is_dir() => stack.push(path),
                Ok(t)
                    if t.is_file()
                        && path
                            .file_name()
                            .and_then(|n| n.to_str())
                            .is_some_and(crate::scan::is_audio_name) =>
                {
                    out.push(path);
                }
                _ => {}
            }
        }
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_unique_nested_and_audio() {
        let mut seen = std::collections::HashSet::new();
        for i in 0..2_000 {
            let p = rel_path(i);
            assert!(p.components().count() >= 4, "{p:?}");
            assert!(crate::scan::is_audio_name(&p.to_string_lossy()));
            assert!(seen.insert(p));
        }
    }

    #[test]
    fn generates_requested_files_and_skips_readmes() {
        let tmp = tempfile::tempdir().expect("tmp");
        let c = generate_tree(tmp.path(), 300).expect("generate");
        assert_eq!(c.files, 300);
        assert!(c.bytes > 300 * 1_000);
        assert_eq!(list_audio(tmp.path()).len(), 300);
        assert!(tmp.path().join("Pack 0000/readme.txt").exists());
        fs::write(tmp.path().join("Pack 0000/note.wav.asd"), b"x").expect("write");
        assert_eq!(list_audio(tmp.path()).len(), 300);
        // Generated WAVs decode.
        let d = crate::decode::decode_file(
            &tmp.path().join(rel_path(299)),
            &crate::decode::DecodeOptions::default(),
        )
        .expect("decode");
        assert_eq!(d.source.sample_rate, 16_000);
    }

    #[test]
    fn mutate_counts_match_disk() {
        let tmp = tempfile::tempdir().expect("tmp");
        generate_tree(tmp.path(), 1_000).expect("generate");
        let m = mutate(tmp.path(), 1_000).expect("mutate");
        assert_eq!(
            m,
            Mutations {
                changed: 10,
                renamed: 10,
                deleted: 5,
                added: 5
            }
        );
        assert_eq!(list_audio(tmp.path()).len(), 1_000 - 5 + 5);
    }
}
