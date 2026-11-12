//! The golden tier: `labels.csv` (`path,category,kind,bpm,key,note,notes`,
//! empty cell = unknown) in `$SONAR_GOLDEN_DIR`, and the template writer
//! behind `sonar eval --init-golden`.
//!
//! Paths are relative to the golden directory, or absolute. `kind` accepts
//! `loop` and `oneshot` / `one_shot` / `one-shot` / `one shot`; `key` accepts
//! `F minor`, `Fm`, `C#m`, `Ebmaj`, `A` (tonic only) and flats or sharps;
//! `note` is scientific pitch (`A4` = 69, `C4` = 60); `category` is a
//! taxonomy id of `docs/categories.md` (`kick`, `hat`, ...; plurals and the
//! taxonomy's name words such as `hihat`, `percussion` also parse). A cell
//! that does not parse is ignored and reported as a warning.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use crate::classify::{Category, Kind};
use crate::text::music::{parse_key_piece, KeyPiece};
use crate::text::Key;
use crate::{Error, Result};

use super::csv;
use super::dataset::{audio_files, Dataset, Item, Labels, Tier};
use super::similarity::parse_similar_jsonl;

/// Label file name inside the golden directory.
pub const LABELS_FILE: &str = "labels.csv";
/// Similarity judgments next to it (optional, see [`super::similarity`]).
pub const SIMILAR_FILE: &str = "similar.jsonl";
/// Header of the label file.
pub const LABELS_HEADER: [&str; 7] = ["path", "category", "kind", "bpm", "key", "note", "notes"];
/// Default number of files in a template.
pub const TEMPLATE_SIZE: usize = 300;

/// Parses a `kind` cell.
pub fn parse_kind(s: &str) -> Option<Kind> {
    let s: String = s
        .trim()
        .to_ascii_lowercase()
        .chars()
        .filter(|c| !matches!(c, '_' | '-' | ' '))
        .collect();
    match s.as_str() {
        "loop" => Some(Kind::Loop),
        "oneshot" => Some(Kind::OneShot),
        _ => None,
    }
}

/// Parses a `key` cell (see the module docs).
pub fn parse_key(s: &str) -> Option<Key> {
    let joined: String = s.split_whitespace().collect();
    let mut chars = joined.chars();
    let first = chars.next()?.to_ascii_uppercase();
    match parse_key_piece(&format!("{first}{}", chars.as_str()))? {
        KeyPiece::Strong(k) => Some(k),
        KeyPiece::Note { key, .. } => Some(key),
    }
}

/// Parses a `note` cell into a MIDI number (`C-1` = 0 … `G9` = 127).
pub fn parse_note(s: &str) -> Option<u8> {
    let s = s.trim();
    let mut chars = s.chars();
    let base: i32 = match chars.next()?.to_ascii_lowercase() {
        'c' => 0,
        'd' => 2,
        'e' => 4,
        'f' => 5,
        'g' => 7,
        'a' => 9,
        'b' => 11,
        _ => return None,
    };
    let rest = chars.as_str();
    let (shift, rest) = match rest.chars().next() {
        Some('#' | '♯') => (1, &rest[rest.chars().next()?.len_utf8()..]),
        Some('b' | '♭') => (-1, &rest[rest.chars().next()?.len_utf8()..]),
        _ => (0, rest),
    };
    // `i8` bounds the octave so the arithmetic below cannot overflow.
    let octave = i32::from(rest.parse::<i8>().ok()?);
    u8::try_from((octave + 1) * 12 + base + shift)
        .ok()
        .filter(|&m| m < 128)
}

/// Loads `dir/labels.csv`.
///
/// # Errors
/// The file cannot be read, or has no `path` column.
pub fn golden_dataset(dir: &Path) -> Result<Dataset> {
    let csv_path = dir.join(LABELS_FILE);
    let text = std::fs::read_to_string(&csv_path).map_err(|source| Error::Io {
        path: csv_path.clone(),
        source,
    })?;
    let rows = csv::parse(&text);
    let header: Vec<String> = rows
        .first()
        .map(|h| h.iter().map(|c| c.trim().to_ascii_lowercase()).collect())
        .unwrap_or_default();
    let col = |name: &str| header.iter().position(|h| h == name);
    let Some(c_path) = col("path") else {
        return Err(Error::Io {
            path: csv_path,
            source: std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "labels.csv has no `path` column",
            ),
        });
    };
    let (c_cat, c_kind, c_bpm, c_key, c_note) = (
        col("category"),
        col("kind"),
        col("bpm"),
        col("key"),
        col("note"),
    );
    let mut ds = Dataset {
        tier: Tier::Golden,
        source: dir.display().to_string(),
        items: Vec::new(),
        unlabeled: 0,
        missing: Vec::new(),
        warnings: Vec::new(),
        similar_queries: Vec::new(),
        similar_extra: Vec::new(),
    };
    for (i, row) in rows.iter().enumerate().skip(1) {
        let cell = |c: Option<usize>| {
            c.and_then(|c| row.get(c))
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
        };
        let Some(rel) = cell(Some(c_path)) else {
            continue;
        };
        let mut warn =
            |what: &str, v: &str| ds.warnings.push(format!("row {}: bad {what} {v:?}", i + 1));
        let mut labels = Labels::default();
        if let Some(v) = cell(c_cat) {
            labels.category = Category::parse(v);
            if labels.category.is_none() {
                warn("category", v);
            }
        }
        if let Some(v) = cell(c_kind) {
            labels.kind = parse_kind(v);
            if labels.kind.is_none() {
                warn("kind", v);
            }
        }
        if let Some(v) = cell(c_bpm) {
            labels.bpm = v.parse::<f64>().ok().filter(|b| b.is_finite() && *b > 0.0);
            if labels.bpm.is_none() {
                warn("bpm", v);
            }
        }
        if let Some(v) = cell(c_key) {
            labels.key = parse_key(v);
            if labels.key.is_none() {
                warn("key", v);
            }
        }
        if let Some(v) = cell(c_note) {
            labels.note_midi = parse_note(v);
            if labels.note_midi.is_none() {
                warn("note", v);
            }
        }
        if !labels.any_scored() {
            ds.unlabeled += 1;
            continue;
        }
        let path = dir.join(rel);
        if path.is_file() {
            ds.items.push(Item {
                id: rel.to_string(),
                path,
                labels,
            });
        } else {
            ds.missing.push(rel.to_string());
        }
    }
    load_similar(&mut ds, dir);
    Ok(ds)
}

/// Reads `dir/similar.jsonl` if present: its queries, and the files it
/// names that `labels.csv` does not (they join the similarity pool).
/// Unreadable or malformed lines become warnings.
fn load_similar(ds: &mut Dataset, dir: &Path) {
    let path = dir.join(SIMILAR_FILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
        Err(e) => {
            ds.warnings.push(format!("{}: {e}", path.display()));
            return;
        }
    };
    let (queries, warnings) = parse_similar_jsonl(&text, dir);
    ds.warnings.extend(warnings);
    let mut known: HashSet<PathBuf> = ds.items.iter().map(|i| i.path.clone()).collect();
    for p in queries
        .iter()
        .flat_map(|q| std::iter::once(&q.query).chain(q.relevant.iter()))
    {
        if p.is_file() && known.insert(p.clone()) {
            let id = p
                .strip_prefix(dir)
                .unwrap_or(p)
                .to_string_lossy()
                .into_owned();
            ds.similar_extra.push(Item {
                id,
                path: p.clone(),
                labels: Labels::default(),
            });
        }
    }
    ds.similar_queries = queries;
}

/// FNV-1a, for a stable pseudo-random order that does not depend on
/// directory enumeration.
fn fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Stratified sample of up to `n` files: files are grouped by parent
/// folder, shuffled deterministically within each group, and taken one per
/// group in rotation (folders sorted by name), so small folders are not
/// drowned by big ones. Result sorted by path.
pub fn stratified_sample(files: &[PathBuf], n: usize) -> Vec<PathBuf> {
    let mut groups: BTreeMap<&Path, Vec<&PathBuf>> = BTreeMap::new();
    for f in files {
        groups
            .entry(f.parent().unwrap_or(Path::new("")))
            .or_default()
            .push(f);
    }
    for g in groups.values_mut() {
        g.sort_by_key(|p| fnv(&p.to_string_lossy()));
    }
    let mut out = Vec::new();
    let mut round = 0;
    while out.len() < n {
        let before = out.len();
        for g in groups.values() {
            if out.len() < n {
                if let Some(p) = g.get(round) {
                    out.push((*p).clone());
                }
            }
        }
        if out.len() == before {
            break;
        }
        round += 1;
    }
    out.sort();
    out
}

/// Writes a `labels.csv` template (header + one row per sampled audio file
/// under `src`, absolute paths, all labels empty) to `golden_dir`; returns
/// the file written and its row count.
///
/// # Errors
/// `src` unreadable, or `golden_dir/labels.csv` already exists (never
/// overwritten).
pub fn init_template(src: &Path, golden_dir: &Path, n: usize) -> Result<(PathBuf, usize)> {
    let out = golden_dir.join(LABELS_FILE);
    let io = |path: &Path, source| Error::Io {
        path: path.to_path_buf(),
        source,
    };
    if out.exists() {
        return Err(io(
            &out,
            std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "refusing to overwrite an existing labels.csv",
            ),
        ));
    }
    let src = std::fs::canonicalize(src).map_err(|e| io(src, e))?;
    let sample = stratified_sample(&audio_files(&src)?, n);
    let mut text = LABELS_HEADER.join(",");
    text.push('\n');
    for p in &sample {
        text.push_str(&csv::escape(&p.to_string_lossy()));
        text.push_str(",,,,,,\n");
    }
    std::fs::create_dir_all(golden_dir).map_err(|e| io(golden_dir, e))?;
    std::fs::write(&out, text).map_err(|e| io(&out, e))?;
    Ok((out, sample.len()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::Mode;

    #[test]
    fn kind_spellings() {
        for s in ["oneshot", "one_shot", "One-Shot", "one shot", " ONESHOT "] {
            assert_eq!(parse_kind(s), Some(Kind::OneShot), "{s}");
        }
        assert_eq!(parse_kind("Loop"), Some(Kind::Loop));
        assert_eq!(parse_kind("pad"), None);
    }

    #[test]
    fn key_spellings() {
        let k = |root, mode| Some(Key { root, mode });
        let (min, maj) = (Some(Mode::Minor), Some(Mode::Major));
        assert_eq!(parse_key("F minor"), k(5, min));
        assert_eq!(parse_key("Fm"), k(5, min));
        assert_eq!(parse_key("C#m"), k(1, min));
        assert_eq!(parse_key("Db minor"), k(1, min));
        assert_eq!(parse_key("Eb major"), k(3, maj));
        assert_eq!(parse_key("a min"), k(9, min));
        assert_eq!(parse_key("A"), k(9, None));
        assert_eq!(parse_key("H"), None);
        assert_eq!(parse_key(""), None);
    }

    #[test]
    fn note_names() {
        assert_eq!(parse_note("A4"), Some(69));
        assert_eq!(parse_note("C4"), Some(60));
        assert_eq!(parse_note("c#3"), Some(49));
        assert_eq!(parse_note("Bb2"), Some(46));
        assert_eq!(parse_note("C-1"), Some(0));
        assert_eq!(parse_note("G9"), Some(127));
        assert_eq!(parse_note("G#9"), None);
        assert_eq!(parse_note("A"), None);
        assert_eq!(parse_note("x4"), None);
    }

    #[test]
    fn loads_labels_with_warnings_and_missing_files() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        std::fs::create_dir_all(d.join("Loops")).unwrap();
        for f in ["Loops/a.wav", "k.wav", "u.wav"] {
            std::fs::write(d.join(f), b"x").unwrap();
        }
        let csv = "path,category,kind,bpm,key,note,notes\n\
            Loops/a.wav,pad,loop,122,F minor,,\"nice, warm\"\n\
            k.wav,kick,one_shot,,,A4,\n\
            u.wav,,,,,,\n\
            gone.wav,,loop,100,,,\n\
            k.wav,,oneshot,fast,H,x,\n\
            u.wav,Hi-Hats,,,,,\n\
            u.wav,banjo,,,,,\n";
        std::fs::write(d.join(LABELS_FILE), csv).unwrap();
        let ds = golden_dataset(d).unwrap();
        assert_eq!(ds.items.len(), 4, "{ds:?}");
        assert_eq!(ds.unlabeled, 2);
        assert_eq!(ds.missing, ["gone.wav"]);
        assert_eq!(ds.warnings.len(), 4, "{:?}", ds.warnings);
        let a = &ds.items[0].labels;
        assert_eq!((a.kind, a.bpm), (Some(Kind::Loop), Some(122.0)));
        assert_eq!(a.category, Some(Category::Pad));
        assert_eq!(ds.items[1].labels.category, Some(Category::Kick));
        assert_eq!(ds.items[1].labels.note_midi, Some(69));
        assert_eq!(ds.items[3].labels.category, Some(Category::Hat));
        assert!(golden_dataset(&d.join("nope")).is_err());
    }

    #[test]
    fn sample_is_stratified_deterministic_and_bounded() {
        let mut files = Vec::new();
        for i in 0..50 {
            files.push(PathBuf::from(format!("/lib/big/{i}.wav")));
        }
        files.push(PathBuf::from("/lib/small/x.wav"));
        files.push(PathBuf::from("/lib/small/y.wav"));
        let s = stratified_sample(&files, 6);
        assert_eq!(s.len(), 6);
        assert_eq!(s.iter().filter(|p| p.starts_with("/lib/small")).count(), 2);
        assert_eq!(s, stratified_sample(&files, 6));
        assert_eq!(stratified_sample(&files, 1000).len(), 52);
    }

    #[test]
    fn template_is_written_once() {
        let tmp = tempfile::tempdir().unwrap();
        let (src, out) = (tmp.path().join("lib"), tmp.path().join("golden"));
        std::fs::create_dir_all(src.join("Kicks")).unwrap();
        std::fs::write(src.join("Kicks/a, b.wav"), b"x").unwrap();
        std::fs::write(src.join("Kicks/c.txt"), b"x").unwrap();
        let (path, n) = init_template(&src, &out, 300).unwrap();
        assert_eq!(n, 1);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("path,category,kind,bpm,key,note,notes\n\""));
        assert!(text.ends_with(",,,,,,\n"));
        // The template loads (no labels yet: nothing to analyse).
        let ds = golden_dataset(&out).unwrap();
        assert_eq!((ds.items.len(), ds.unlabeled), (0, 1));
        let again = init_template(&src, &out, 300);
        assert!(matches!(again, Err(Error::Io { .. })));
    }
}
