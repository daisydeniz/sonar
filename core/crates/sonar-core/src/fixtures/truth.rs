//! Truth values of `fixtures.json` entries (re-exported by
//! [`manifest`](super::manifest)). Every field is optional and omitted when
//! it does not apply; the per-field legend is
//! [`fields_legend`](super::legend::fields_legend).

use serde::{Deserialize, Serialize};

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
}
