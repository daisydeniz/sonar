//! `fixtures.json`: the truth manifest written next to the fixture files.
//!
//! The manifest is the **only** place truth lives (file names are neutral).
//! Schema documented in `docs/EVAL.md` ("Fixture manifest") and, per field,
//! in the manifest's own `fields` legend
//! ([`fields_legend`](super::legend::fields_legend)). Truth types live in
//! [`truth`](super::truth) and are re-exported here.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

pub use super::truth::{
    Category, ChordTruth, CorruptTruth, Expect, KeyTruth, Kind, Mode, NoiseColor, Truth,
};
use crate::{Error, Result};

/// Bumped on any incompatible change to the manifest layout.
///
/// 2: `format.container`, `format.codec` and `format.lossless` added;
/// `format.bit_depth`, `sample_format` and `format_tag` became optional;
/// new group `containers`.
pub const SCHEMA_VERSION: u32 = 2;

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
    /// The A4 sine in every bit depth × rate × channel combination (WAV).
    Formats,
    /// Every non-WAV container the scanner accepts (AIFF, FLAC, M4A/ALAC,
    /// MP3, Ogg Vorbis).
    Containers,
    /// Deliberately broken files.
    Corrupt,
}

impl Group {
    /// Every group, in catalog order.
    pub const ALL: [Group; 9] = [
        Self::Tonal,
        Self::Chords,
        Self::Loops,
        Self::Oneshots,
        Self::Noise,
        Self::Edge,
        Self::Formats,
        Self::Containers,
        Self::Corrupt,
    ];

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
            Self::Containers => "containers",
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
    /// File container.
    pub container: Container,
    /// Audio codec inside the container.
    pub codec: Codec,
    /// `true`: decoding must reproduce the written samples bit-exactly.
    /// `false` (lossy): sample-level truth does not apply; only rate,
    /// channels, exact length and the declared level truth (digital
    /// silence for every lossy fixture) are checked.
    pub lossless: bool,
    /// Frames per second (Hz).
    pub sample_rate: u32,
    /// Channel count.
    pub channels: u16,
    /// Bits per sample (16, 24 or 32); absent for lossy codecs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bit_depth: Option<u16>,
    /// Integer PCM or IEEE float; absent for lossy codecs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample_format: Option<SampleKind>,
    /// WAV format tag family; WAV only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format_tag: Option<FormatTag>,
    /// Playable frames: for lossy codecs after removing encoder priming
    /// and padding (what a gapless decoder returns).
    pub num_frames: u64,
    /// `num_frames / sample_rate` (s).
    pub duration_s: f64,
}

/// File container (matches the file extension family).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Container {
    /// RIFF/WAVE (`.wav`).
    Wav,
    /// AIFF (`.aif`, `.aiff`).
    Aiff,
    /// Native FLAC (`.flac`).
    Flac,
    /// MPEG audio elementary stream (`.mp3`).
    Mp3,
    /// Ogg (`.ogg`).
    Ogg,
    /// ISO base media / MPEG-4 audio (`.m4a`).
    M4a,
}

/// Audio codec.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Codec {
    /// Uncompressed PCM (integer or float).
    Pcm,
    /// FLAC.
    Flac,
    /// Apple Lossless.
    Alac,
    /// MPEG-1/2 Audio Layer III.
    Mp3,
    /// Vorbis I.
    Vorbis,
}

impl Codec {
    /// `true` for codecs that reproduce samples bit-exactly.
    pub fn is_lossless(self) -> bool {
        matches!(self, Self::Pcm | Self::Flac | Self::Alac)
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn s<T: Serialize>(v: &T) -> String {
        serde_json::to_string(v).unwrap()
    }

    #[test]
    fn enums_serialize_snake_case() {
        assert_eq!(s(&FormatTag::IeeeFloat), "\"ieee_float\"");
        assert_eq!(s(&SampleKind::Float), "\"float\"");
        assert_eq!(s(&Container::M4a), "\"m4a\"");
        assert_eq!(s(&Codec::Vorbis), "\"vorbis\"");
    }

    #[test]
    fn group_dir_matches_serialized_name() {
        for g in Group::ALL {
            assert_eq!(s(&g), format!("\"{}\"", g.dir()));
        }
    }

    #[test]
    fn lossless_codecs() {
        assert!(Codec::Pcm.is_lossless() && Codec::Flac.is_lossless() && Codec::Alac.is_lossless());
        assert!(!Codec::Mp3.is_lossless() && !Codec::Vorbis.is_lossless());
    }

    #[test]
    fn optional_format_fields_are_omitted() {
        let f = Format {
            container: Container::Mp3,
            codec: Codec::Mp3,
            lossless: false,
            sample_rate: 44_100,
            channels: 2,
            bit_depth: None,
            sample_format: None,
            format_tag: None,
            num_frames: 10,
            duration_s: 0.0,
        };
        let json = s(&f);
        assert!(
            !json.contains("bit_depth") && !json.contains("format_tag"),
            "{json}"
        );
        let back: Format = serde_json::from_str(&json).unwrap();
        assert_eq!(back, f);
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
