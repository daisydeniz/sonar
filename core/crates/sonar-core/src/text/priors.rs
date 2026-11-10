//! Filename priors: BPM and key read from a sample's file name (and, with
//! lower priority, its parent folder names).
//!
//! Uses the same recognition rules as the tokenizer (see
//! [`super::recognize`]), so a prior always matches the `120bpm` /
//! `key:…` tokens of the name. Conservative by design: conflicting values
//! give `None`; folder names only contribute *explicit* BPMs (`124 BPM`)
//! and strong keys (`Am`, `F# minor`), never bare numbers/letters. Analysis
//! weighs a prior against its own estimate; it never overrides user values.

use std::ops::RangeInclusive;
use std::path::{Component, Path};

use serde::Serialize;

use super::lex::{lex, Piece};
use super::music::Key;
use super::recognize::{recognize, NoiseRules, Unit, UnitKind};
use super::tokenize::strip_audio_extension;

/// BPM values outside this range are ignored as priors.
pub const PRIOR_BPM_RANGE: RangeInclusive<f32> = 40.0..=300.0;
/// Parent folders consulted (nearest first) when the name has no prior.
pub const PRIOR_DIR_LEVELS: usize = 2;

/// Where a prior was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PriorSource {
    /// The file name.
    Filename,
    /// A parent folder name.
    Directory,
}

/// A BPM read from a name.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct BpmPrior {
    /// Tempo in beats per minute.
    pub bpm: f32,
    /// `true` if marked with `bpm`; `false` for a bare number accepted by
    /// the tempo-context rule.
    pub explicit: bool,
    /// Where it was found.
    pub source: PriorSource,
}

/// A key read from a name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct KeyPrior {
    /// The key (`mode: None` when only the note was named).
    pub key: Key,
    /// `false` for a bare note letter accepted by the context rule
    /// (`Loop_120_C`).
    pub explicit: bool,
    /// Where it was found.
    pub source: PriorSource,
}

/// BPM and key priors of one file.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct FilenamePrior {
    /// Tempo, if the name states one unambiguously.
    pub bpm: Option<BpmPrior>,
    /// Key, if the name states one unambiguously.
    pub key: Option<KeyPrior>,
}

/// Priors of `path`: file name first, then up to [`PRIOR_DIR_LEVELS`]
/// parent folders (explicit values only) for whatever is still missing.
pub fn filename_prior(path: &Path) -> FilenamePrior {
    let mut pieces = Vec::new();
    let mut units = Vec::new();
    let mut prior = FilenamePrior::default();
    if let Some(name) = path.file_name() {
        let name = name.to_string_lossy();
        let stem = strip_audio_extension(&name);
        let (bpm, key) = segment_prior(stem, &mut pieces, &mut units);
        let source = PriorSource::Filename;
        prior.bpm = bpm.map(|(bpm, explicit)| BpmPrior {
            bpm,
            explicit,
            source,
        });
        prior.key = key.map(|(key, explicit)| KeyPrior {
            key,
            explicit,
            source,
        });
    }
    let dirs = path
        .parent()
        .into_iter()
        .flat_map(Path::components)
        .collect::<Vec<_>>();
    for dir in dirs.iter().rev().take(PRIOR_DIR_LEVELS) {
        if prior.bpm.is_some() && prior.key.is_some() {
            break;
        }
        let Component::Normal(dir) = dir else {
            continue;
        };
        let (bpm, key) = segment_prior(&dir.to_string_lossy(), &mut pieces, &mut units);
        let source = PriorSource::Directory;
        if prior.bpm.is_none() {
            prior.bpm = bpm
                .filter(|&(_, explicit)| explicit)
                .map(|(bpm, explicit)| BpmPrior {
                    bpm,
                    explicit,
                    source,
                });
        }
        if prior.key.is_none() {
            prior.key = key
                .filter(|&(_, explicit)| explicit)
                .map(|(key, explicit)| KeyPrior {
                    key,
                    explicit,
                    source,
                });
        }
    }
    prior
}

/// BPM and key of one name, each with its `explicit` flag.
type NamePrior = (Option<(f32, bool)>, Option<(Key, bool)>);

/// Priors of one file or folder name.
fn segment_prior(name: &str, pieces: &mut Vec<Piece>, units: &mut Vec<Unit>) -> NamePrior {
    // A component has no `/`, but may contain `\` (a segment break for the
    // lexer): only the last segment counts, like a file name.
    lex(name, pieces);
    let rules = NoiseRules {
        drop_padded_numbers: true,
        drop_versions: true,
        query: false,
    };
    recognize(name, pieces, rules, units);
    let last_segment = units.last().map_or(0, |u| u.segment);
    let units = units.iter().filter(|u| u.segment == last_segment);
    (bpm_of(units.clone()), key_of(units))
}

fn bpm_of<'a>(units: impl Iterator<Item = &'a Unit> + Clone) -> Option<(f32, bool)> {
    let found = |want_explicit: bool| {
        let mut values = units.clone().filter_map(move |u| match u.kind {
            UnitKind::Bpm { value, explicit }
                if explicit == want_explicit && PRIOR_BPM_RANGE.contains(&value) =>
            {
                Some(value)
            }
            _ => None,
        });
        let first = values.next()?;
        // Conflicting values: no prior.
        values.all(|v| v == first).then_some(first)
    };
    found(true)
        .map(|v| (v, true))
        .or_else(|| found(false).map(|v| (v, false)))
}

fn key_of<'a>(units: impl Iterator<Item = &'a Unit> + Clone) -> Option<(Key, bool)> {
    let found = |want_bare: bool| {
        let mut merged: Option<Key> = None;
        for u in units.clone() {
            let UnitKind::Key { key, bare } = u.kind else {
                continue;
            };
            if bare != want_bare {
                continue;
            }
            // Same root merges (a stated mode wins); a conflict gives None.
            merged = match merged {
                None => Some(key),
                Some(m) if m.root != key.root => return None,
                Some(m) => match (m.mode, key.mode) {
                    (Some(a), Some(b)) if a != b => return None,
                    (None, _) => Some(key),
                    _ => Some(m),
                },
            };
        }
        merged
    };
    found(false)
        .map(|k| (k, true))
        .or_else(|| found(true).map(|k| (k, false)))
}
