//! Catalog entry types: what each fixture file contains and how it is
//! written. The list itself lives in [`catalog`](super::catalog).

use super::corrupt::CorruptKind;
use super::drums::Pattern;
use super::manifest::{Group, Kind, NoiseColor};
use super::music::Key;
use super::oneshot::DrumHit;
use super::wav::WavSpec;

/// What to synthesise.
#[derive(Debug, Clone, PartialEq)]
pub enum Content {
    /// Pure sine at a MIDI note with ≤ 10 ms fades (`kind` tone or one_shot).
    Sine {
        /// MIDI note.
        midi: u8,
        /// Length (s).
        secs: f64,
        /// Kind truth.
        kind: Kind,
    },
    /// Plucked additive tone (6 partials, exponential decay).
    Pluck {
        /// MIDI note.
        midi: u8,
        /// Length (s).
        secs: f64,
    },
    /// Sustained pad with slow tremolo (used for the > 5 min file).
    Pad {
        /// MIDI note.
        midi: u8,
        /// Length (s).
        secs: f64,
    },
    /// Hard-clipped sine (`drive_db` above full scale).
    ClippedSine {
        /// MIDI note.
        midi: u8,
        /// Length (s).
        secs: f64,
        /// Gain before the clipper (dB).
        drive_db: f64,
    },
    /// Chord-progression loop in a key (4/4).
    Chords {
        /// Key.
        key: Key,
        /// Tempo (BPM).
        bpm: f64,
        /// Bars.
        bars: u32,
    },
    /// Drum loop (4/4).
    DrumLoop {
        /// Tempo (BPM).
        bpm: f64,
        /// Bars.
        bars: u32,
        /// Pattern.
        pattern: Pattern,
    },
    /// Metronome loop.
    ClickLoop {
        /// Tempo (BPM).
        bpm: f64,
        /// Bars.
        bars: u32,
        /// Beats per bar.
        beats_per_bar: u32,
    },
    /// Drum one-shot.
    Hit(DrumHit),
    /// Noise; `burst = Some((start_s, len_s))` gates it inside silence.
    Noise {
        /// Spectrum.
        color: NoiseColor,
        /// File length (s).
        secs: f64,
        /// Optional burst window.
        burst: Option<(f64, f64)>,
    },
    /// Digital silence of `frames` frames.
    Silence {
        /// Frames.
        frames: usize,
    },
    /// Single-frame impulse.
    Impulse,
}

/// What a catalog entry produces.
#[derive(Debug, Clone, PartialEq)]
pub enum Body {
    /// A valid WAV.
    Audio {
        /// Header format.
        spec: WavSpec,
        /// Signal.
        content: Content,
        /// Peak normalisation target (dBFS); `None` keeps the raw level.
        peak_dbfs: Option<f64>,
    },
    /// A deliberately broken file.
    Corrupt(CorruptKind),
}

/// One catalog entry.
#[derive(Debug, Clone, PartialEq)]
pub struct FixtureSpec {
    /// Relative path (`group/name.wav`).
    pub path: String,
    /// Group.
    pub group: Group,
    /// Human-readable description.
    pub description: String,
    /// What to write.
    pub body: Body,
}
