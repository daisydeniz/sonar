//! `fixtures.json`: the truth manifest written next to the fixture files.
//!
//! The manifest is the **only** place truth lives (file names are neutral).
//! Schema documented in `docs/EVAL.md` ("Fixture manifest") and, per field,
//! in the manifest's own `fields` legend
//! ([`fields_legend`](super::legend::fields_legend)).

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// Bumped on any incompatible change to the manifest layout.
pub const SCHEMA_VERSION: u32 = 1;

/// Top-level manifest document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    /// Layout version ([`SCHEMA_VERSION`]).
    pub schema_version: u32,
    /// Who wrote the manifest.
    pub generator: Generator,
    /// PRNG seed the noise content was generated from.
    pub seed: u64,
    /// Legend: dotted field path → meaning and unit.
    pub fields: BTreeMap<String, FieldDoc>,
    /// One entry per file, sorted by `path`.
    pub fixtures: Vec<Fixture>,
}

impl Manifest {
    /// Reads and parses a manifest file.
    pub fn read(path: &Path) -> Result<Self> {
        let bytes = std::fs::read(path).map_err(|e| Error::io(path, e))?;
        serde_json::from_slice(&bytes).map_err(|e| Error::json(path, e))
    }

    /// Sum of all fixture file sizes in bytes.
    pub fn total_bytes(&self) -> u64 {
        self.fixtures.iter().map(|f| f.bytes).sum()
    }
}

/// Generator identification.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Generator {
    /// Always `"sonar-core fixtures"`.
    pub name: String,
    /// `sonar-core` crate version.
    pub version: String,
}

/// One legend entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldDoc {
    /// One-line meaning.
    pub description: String,
    /// Unit, if the value is a quantity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
}

/// Fixture group; also the subdirectory the file lives in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Group {
    /// Single notes (sustained sines, plucks).
    Tonal,
    /// Chord progressions in known keys.
    Chords,
    /// Drum and click loops at known tempi.
    Loops,
    /// Drum one-shots.
    Oneshots,
    /// White/pink noise.
    Noise,
    /// Silence, very short, very long, extreme levels.
    Edge,
    /// The A4 sine in every bit depth × rate × channel combination.
    Formats,
    /// Deliberately broken files.
    Corrupt,
}

impl Group {
    /// Directory name (same as the serialized name).
    pub fn dir(self) -> &'static str {
        match self {
            Self::Tonal => "tonal",
            Self::Chords => "chords",
            Self::Loops => "loops",
            Self::Oneshots => "oneshots",
            Self::Noise => "noise",
            Self::Edge => "edge",
            Self::Formats => "formats",
            Self::Corrupt => "corrupt",
        }
    }
}

/// One fixture file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fixture {
    /// Path relative to the fixture directory, `/`-separated.
    pub path: String,
    /// Group (= subdirectory).
    pub group: Group,
    /// Human-readable description (never read by analysis code).
    pub description: String,
    /// File size in bytes.
    pub bytes: u64,
    /// Audio format as written; absent for corrupt files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<Format>,
    /// Truth values.
    pub truth: Truth,
}

/// Audio format of a valid fixture.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Format {
    /// Frames per second (Hz).
    pub sample_rate: u32,
    /// Channel count.
    pub channels: u16,
    /// Bits per sample (16, 24 or 32).
    pub bit_depth: u16,
    /// Integer PCM or IEEE float.
    pub sample_format: SampleKind,
    /// WAV format tag family.
    pub format_tag: FormatTag,
    /// Frames in the `data` chunk.
    pub num_frames: u64,
    /// `num_frames / sample_rate` (s).
    pub duration_s: f64,
}

/// Sample representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SampleKind {
    /// Signed integer PCM.
    Int,
    /// IEEE float.
    Float,
}

/// WAV `fmt ` format tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FormatTag {
    /// `WAVE_FORMAT_PCM` (1).
    Pcm,
    /// `WAVE_FORMAT_IEEE_FLOAT` (3).
    IeeeFloat,
    /// `WAVE_FORMAT_EXTENSIBLE` (0xFFFE).
    Extensible,
}

/// Truth values; every field is optional and omitted when not applicable.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Truth {
    /// What kind of sound this is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<Kind>,
    /// Instrument/sound category.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<Category>,
    /// Tempo (beats per minute) of a loop.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bpm: Option<f64>,
    /// Whole bars in the loop.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bars: Option<u32>,
    /// Beats per bar (time signature numerator, quarter-note beats).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub beats_per_bar: Option<u32>,
    /// Musical key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<KeyTruth>,
    /// Chords in time order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chords: Option<Vec<ChordTruth>>,
    /// MIDI note number of the fundamental (69 = A4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note_midi: Option<u8>,
    /// Scientific pitch name, e.g. `"F#3"` (C4 = MIDI 60).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note_name: Option<String>,
    /// Fundamental frequency (Hz).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pitch_hz: Option<f64>,
    /// Noise spectrum.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub noise_color: Option<NoiseColor>,
    /// Sample peak of the written samples (dBFS); absent for silence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peak_dbfs: Option<f64>,
    /// RMS of the written samples (dBFS); absent for silence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rms_dbfs: Option<f64>,
    /// Every written sample is exactly zero.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_silent: Option<bool>,
    /// Present only for deliberately corrupt files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corrupt: Option<CorruptTruth>,
}

/// Sound kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Sustained pitched tone (not a loop, not percussive).
    Tone,
    /// Single hit / note with a decaying envelope.
    OneShot,
    /// Tempo-synced, seamlessly loopable musical phrase.
    Loop,
    /// Noise (steady or burst).
    Noise,
    /// Digital silence.
    Silence,
}

/// Sound category (fixture vocabulary; mapped to classifier labels by eval).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    /// Kick drum.
    Kick,
    /// Snare drum.
    Snare,
    /// Closed hi-hat.
    HatClosed,
    /// Open hi-hat.
    HatOpen,
    /// Drum loop.
    Drums,
    /// Metronome click loop.
    Click,
    /// Pitched synth (plucks, chord loops, pads).
    Synth,
    /// Pure sine test tone.
    Sine,
    /// Noise.
    Noise,
}

/// Major or minor (keys and triads).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// Major.
    Major,
    /// Minor.
    Minor,
}

/// Key truth.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyTruth {
    /// Tonic, canonical spelling (C C# D Eb E F F# G Ab A Bb B).
    pub tonic: String,
    /// Tonic pitch class, 0 = C … 11 = B.
    pub tonic_pc: u8,
    /// Mode.
    pub mode: Mode,
}

/// One chord of a progression.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChordTruth {
    /// Chord symbol: root + `"m"` for minor, e.g. `"F#m"`, `"Bb"`.
    pub symbol: String,
    /// Root, canonical spelling.
    pub root: String,
    /// Triad quality.
    pub quality: Mode,
    /// Roman numeral relative to the key (`"V"` is major in minor keys).
    pub roman: String,
    /// Start time (s).
    pub start_s: f64,
    /// Length (s).
    pub duration_s: f64,
}

/// Noise spectrum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoiseColor {
    /// Flat spectrum.
    White,
    /// −3 dB/octave.
    Pink,
}

/// Truth for a corrupt file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CorruptTruth {
    /// Machine-readable defect id, e.g. `"truncated_header"`.
    pub reason: String,
    /// What a robust decoder must do.
    pub expect: Expect,
}

/// Required decoder behaviour on a corrupt file. In every case the decoder
/// must not panic, hang, or allocate unboundedly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Expect {
    /// Must return an error.
    Error,
    /// May return an error or decode some/all of the audio.
    ErrorOrPartial,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s<T: Serialize>(v: &T) -> String {
        serde_json::to_string(v).unwrap()
    }

    #[test]
    fn enums_serialize_snake_case() {
        assert_eq!(s(&Kind::OneShot), "\"one_shot\"");
        assert_eq!(s(&Category::HatClosed), "\"hat_closed\"");
        assert_eq!(s(&Expect::ErrorOrPartial), "\"error_or_partial\"");
        assert_eq!(s(&FormatTag::IeeeFloat), "\"ieee_float\"");
        assert_eq!(s(&SampleKind::Float), "\"float\"");
    }

    #[test]
    fn group_dir_matches_serialized_name() {
        for g in [
            Group::Tonal,
            Group::Chords,
            Group::Loops,
            Group::Oneshots,
            Group::Noise,
            Group::Edge,
            Group::Formats,
            Group::Corrupt,
        ] {
            assert_eq!(s(&g), format!("\"{}\"", g.dir()));
        }
    }

    #[test]
    fn empty_truth_serializes_to_empty_object_and_round_trips() {
        assert_eq!(s(&Truth::default()), "{}");
        let t = Truth {
            kind: Some(Kind::Loop),
            bpm: Some(122.5),
            ..Truth::default()
        };
        let back: Truth = serde_json::from_str(&s(&t)).unwrap();
        assert_eq!(back, t);
    }

    #[test]
    fn read_reports_missing_and_invalid_files() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nope.json");
        assert!(matches!(Manifest::read(&missing), Err(Error::Io { .. })));
        let bad = dir.path().join("bad.json");
        std::fs::write(&bad, b"{").unwrap();
        assert!(matches!(Manifest::read(&bad), Err(Error::Json { .. })));
    }
}
