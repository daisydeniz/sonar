//! Evaluation datasets: files with their labels.
//!
//! - **Fixtures**: truth from `fixtures.json`. Kind mapping (ADR-020, same
//!   as the T3.3 test): `loop` → Loop; `one_shot`, `tone`, `noise` →
//!   OneShot; `silence` has no kind label. BPM is labeled on loops,
//!   key on every file with `truth.key`, note on tones/one-shots with
//!   `truth.note_midi`. Corrupt files carry no labels, only the
//!   expectation that decoding fails (or may).
//! - **Silver**: labels read from the relative path with the product's own
//!   filename priors ([`filename_prior`], [`kind_hint`]).
//! - **Golden**: `labels.csv` (see [`super::golden`]).
//!
//! The analyzer never sees these labels or the file names (see
//! [`super::run`]).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::classify::{kind_hint, Kind};
use crate::fixtures::manifest::{Kind as TruthKind, Manifest, Mode as TruthMode};
use crate::scan::is_audio_name;
use crate::text::{filename_prior, Key, Mode};
use crate::{Error, Result};

use super::similarity::{fixture_family, SimilarQuery};

/// A dataset tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    /// Synthetic fixtures with exact truth.
    Fixtures,
    /// Real files with labels in their names.
    Silver,
    /// Hand-labeled real files.
    Golden,
}

impl Tier {
    /// Lowercase name used in reports and the baseline.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fixtures => "fixtures",
            Self::Silver => "silver",
            Self::Golden => "golden",
        }
    }
}

/// Labels of one file; `None` = unknown (the file is excluded from that
/// metric).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Labels {
    /// Loop or one-shot.
    pub kind: Option<Kind>,
    /// Loop tempo (BPM).
    pub bpm: Option<f64>,
    /// Key (`mode: None` = only the tonic is known).
    pub key: Option<Key>,
    /// MIDI number of a tonal one-shot's note.
    pub note_midi: Option<u8>,
    /// Category (kept for the classifier metrics, not scored yet).
    pub category: Option<String>,
    /// Decoding is expected to fail (corrupt fixture).
    pub expect_error: bool,
    /// Fixture sound family: relevance class of the similarity metric
    /// ([`super::similarity::fixture_family`]); `None` = not in the pool.
    pub family: Option<String>,
}

impl Labels {
    /// True if any scored label is present.
    pub fn any_scored(&self) -> bool {
        self.kind.is_some()
            || self.bpm.is_some()
            || self.key.is_some()
            || self.note_midi.is_some()
            || self.expect_error
    }
}

/// One file to analyse.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    /// Display name (path relative to the dataset root, `/`-separated).
    pub id: String,
    /// File to read.
    pub path: PathBuf,
    /// Truth.
    pub labels: Labels,
}

/// A loaded tier.
#[derive(Debug, Clone, PartialEq)]
pub struct Dataset {
    /// Which tier.
    pub tier: Tier,
    /// Where the files came from (shown in the report).
    pub source: String,
    /// Files with at least one label (these are analysed).
    pub items: Vec<Item>,
    /// Files seen without any usable label (skipped).
    pub unlabeled: usize,
    /// Labeled entries whose file does not exist (golden only).
    pub missing: Vec<String>,
    /// Label cells that could not be parsed (ignored), e.g. `row 7: bad key "H"`.
    pub warnings: Vec<String>,
    /// Golden similarity queries (`similar.jsonl`); empty = relevance comes
    /// from the items' fixture family.
    pub similar_queries: Vec<SimilarQuery>,
    /// Files named only by `similar_queries`: analysed for the similarity
    /// pool, never scored for the other metrics.
    pub similar_extra: Vec<Item>,
}

impl Dataset {
    /// True if similarity can be measured (fixture families or golden
    /// queries).
    pub fn has_similarity(&self) -> bool {
        !self.similar_queries.is_empty() || self.items.iter().any(|i| i.labels.family.is_some())
    }
}

/// Fixture truth → loop/one-shot label (ADR-020).
pub fn fixture_kind(truth: TruthKind) -> Option<Kind> {
    match truth {
        TruthKind::Loop => Some(Kind::Loop),
        TruthKind::OneShot | TruthKind::Tone | TruthKind::Noise => Some(Kind::OneShot),
        TruthKind::Silence => None,
    }
}

/// Dataset of a generated fixture directory (`dir` holds the files named in
/// `manifest`).
pub fn fixtures_dataset(manifest: &Manifest, dir: &Path) -> Dataset {
    let mut items = Vec::new();
    let mut unlabeled = 0;
    for f in &manifest.fixtures {
        let t = &f.truth;
        let tonal = matches!(t.kind, Some(TruthKind::Tone | TruthKind::OneShot));
        let labels = Labels {
            kind: t.kind.and_then(fixture_kind),
            bpm: t.bpm.filter(|_| t.kind == Some(TruthKind::Loop)),
            key: t.key.as_ref().map(|k| Key {
                root: k.tonic_pc,
                mode: Some(match k.mode {
                    TruthMode::Major => Mode::Major,
                    TruthMode::Minor => Mode::Minor,
                }),
            }),
            note_midi: t.note_midi.filter(|_| tonal),
            category: None,
            expect_error: t.corrupt.is_some(),
            family: fixture_family(f).map(str::to_string),
        };
        if labels.any_scored() {
            items.push(Item {
                id: f.path.clone(),
                path: dir.join(&f.path),
                labels,
            });
        } else {
            unlabeled += 1;
        }
    }
    Dataset {
        tier: Tier::Fixtures,
        source: "generated (sonar fixtures, default seed)".into(),
        items,
        unlabeled,
        missing: Vec::new(),
        warnings: Vec::new(),
        similar_queries: Vec::new(),
        similar_extra: Vec::new(),
    }
}

/// Labels read from a relative path, as the product reads names: BPM and
/// key from [`filename_prior`], loop/one-shot from [`kind_hint`] (a BPM
/// marker or loop word means loop; `oneshot`, `hit` mean one-shot).
pub fn silver_labels(rel: &Path) -> Labels {
    let prior = filename_prior(rel);
    Labels {
        kind: kind_hint(rel),
        bpm: prior.bpm.map(|b| f64::from(b.bpm)),
        key: prior.key.map(|k| k.key),
        ..Labels::default()
    }
}

/// Silver dataset: every audio file under `root` whose relative path
/// carries a label.
///
/// # Errors
/// `root` cannot be read.
pub fn silver_dataset(root: &Path) -> Result<Dataset> {
    let mut items = Vec::new();
    let mut unlabeled = 0;
    for path in audio_files(root)? {
        let Ok(rel) = path.strip_prefix(root) else {
            continue;
        };
        let labels = silver_labels(rel);
        if labels.any_scored() {
            items.push(Item {
                id: rel_id(rel),
                path: path.clone(),
                labels,
            });
        } else {
            unlabeled += 1;
        }
    }
    Ok(Dataset {
        tier: Tier::Silver,
        source: root.display().to_string(),
        items,
        unlabeled,
        missing: Vec::new(),
        warnings: Vec::new(),
        similar_queries: Vec::new(),
        similar_extra: Vec::new(),
    })
}

/// `/`-separated display form of a relative path.
fn rel_id(rel: &Path) -> String {
    rel.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// Audio files under `root` (recursive, symlinks not followed), sorted.
///
/// # Errors
/// `root` cannot be read; unreadable subdirectories are skipped.
pub fn audio_files(root: &Path) -> Result<Vec<PathBuf>> {
    let io = |source| Error::Io {
        path: root.to_path_buf(),
        source,
    };
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    let mut first = true;
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) if first => return Err(io(e)),
            Err(_) => continue,
        };
        first = false;
        for entry in entries.flatten() {
            let Ok(ty) = entry.file_type() else { continue };
            let path = entry.path();
            if ty.is_dir() {
                stack.push(path);
            } else if ty.is_file() && is_audio_name(&entry.file_name().to_string_lossy()) {
                out.push(path);
            }
        }
    }
    out.sort();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rel(s: &str) -> Labels {
        silver_labels(Path::new(s))
    }

    #[test]
    fn silver_labels_come_from_the_name() {
        let l = rel("Packs/Pad_Loop_124bpm_Fmin_01.wav");
        assert_eq!(l.kind, Some(Kind::Loop));
        assert_eq!(l.bpm, Some(124.0));
        assert_eq!(
            l.key,
            Some(Key {
                root: 5,
                mode: Some(Mode::Minor)
            })
        );
        let l = rel("Drums/Kick_OneShot_03.wav");
        assert_eq!((l.kind, l.bpm, l.key), (Some(Kind::OneShot), None, None));
        assert!(!rel("misc/sample_17.wav").any_scored());
    }

    #[test]
    fn fixture_kinds_map_per_adr_020() {
        assert_eq!(fixture_kind(TruthKind::Loop), Some(Kind::Loop));
        assert_eq!(fixture_kind(TruthKind::Tone), Some(Kind::OneShot));
        assert_eq!(fixture_kind(TruthKind::Noise), Some(Kind::OneShot));
        assert_eq!(fixture_kind(TruthKind::Silence), None);
    }

    #[test]
    fn walk_finds_audio_only_sorted() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("b/c")).unwrap();
        for f in ["b/c/x.wav", "a.FLAC", "notes.txt", "b/y.mp3"] {
            std::fs::write(tmp.path().join(f), b"x").unwrap();
        }
        let got: Vec<String> = audio_files(tmp.path())
            .unwrap()
            .iter()
            .map(|p| rel_id(p.strip_prefix(tmp.path()).unwrap()))
            .collect();
        assert_eq!(got, ["a.FLAC", "b/c/x.wav", "b/y.mp3"]);
        assert!(audio_files(&tmp.path().join("nope")).is_err());
    }
}
