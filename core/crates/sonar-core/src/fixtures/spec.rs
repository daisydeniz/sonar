//! Catalog entry types: what each fixture file contains and how it is
//! written. The list itself lives in [`catalog`](super::catalog) and
//! [`containers`](super::containers).

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

/// How a rendered signal is stored losslessly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Store {
    /// RIFF/WAVE, exactly as [`WavSpec`] says (int or float).
    Wav,
    /// AIFF (integer PCM; float specs are stored as 24-bit).
    Aiff,
    /// FLAC (integer PCM; float specs are stored as 24-bit).
    Flac,
    /// ALAC in `.m4a` (integer PCM; float specs are stored as 24-bit).
    Alac,
}

/// Lossy codecs the fixture writers produce (digital silence only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LossyCodec {
    /// MPEG Layer III with Xing/Info + LAME tag (`.mp3`).
    Mp3,
    /// Vorbis I in Ogg (`.ogg`).
    Vorbis,
}

/// What a catalog entry produces.
#[derive(Debug, Clone, PartialEq)]
pub enum Body {
    /// A valid lossless file.
    Audio {
        /// Rate, channels and sample format (the WAV header for
        /// [`Store::Wav`]).
        spec: WavSpec,
        /// Signal.
        content: Content,
        /// Peak normalisation target (dBFS); `None` keeps the raw level.
        peak_dbfs: Option<f64>,
        /// Container/codec.
        store: Store,
    },
    /// A valid lossy stream of digital silence.
    LossySilence {
        /// Codec (and thereby container).
        codec: LossyCodec,
        /// Frames per second.
        sample_rate: u32,
        /// Channel count (1 or 2).
        channels: u16,
        /// Playable frames (after gapless trimming).
        frames: u64,
    },
    /// A deliberately broken file.
    Corrupt(CorruptKind),
}

/// One catalog entry.
#[derive(Debug, Clone, PartialEq)]
pub struct FixtureSpec {
    /// Relative path (`group/name.ext`).
    pub path: String,
    /// Group.
    pub group: Group,
    /// Human-readable description.
    pub description: String,
    /// What to write.
    pub body: Body,
}

/// Appends catalog entries for one group at a time, numbering files
/// `<group>/<prefix>_NNN.<ext>` per group.
#[derive(Debug)]
pub(super) struct Builder {
    /// Entries so far.
    pub out: Vec<FixtureSpec>,
    group: Group,
    prefix: &'static str,
}

impl Builder {
    /// Empty builder.
    pub fn new() -> Self {
        Self {
            out: Vec::new(),
            group: Group::Tonal,
            prefix: "tone",
        }
    }

    /// Switches to `group`, naming files `<prefix>_NNN`.
    pub fn group(&mut self, group: Group, prefix: &'static str) {
        self.group = group;
        self.prefix = prefix;
    }

    /// Adds a WAV file.
    pub fn add(
        &mut self,
        spec: WavSpec,
        content: Content,
        peak: Option<f64>,
        desc: impl Into<String>,
    ) {
        self.add_stored(Store::Wav, spec, content, peak, desc);
    }

    /// Adds a lossless file in `store`.
    pub fn add_stored(
        &mut self,
        store: Store,
        spec: WavSpec,
        content: Content,
        peak: Option<f64>,
        desc: impl Into<String>,
    ) {
        let ext = match store {
            Store::Wav => "wav",
            Store::Aiff => "aiff",
            Store::Flac => "flac",
            Store::Alac => "m4a",
        };
        let body = Body::Audio {
            spec,
            content,
            peak_dbfs: peak,
            store,
        };
        self.push_ext(body, desc.into(), ext);
    }

    /// Adds `body` as a `.wav` file.
    pub fn push(&mut self, body: Body, description: String) {
        self.push_ext(body, description, "wav");
    }

    /// Adds `body` with file extension `ext`.
    pub fn push_ext(&mut self, body: Body, description: String, ext: &str) {
        let n = self.out.iter().filter(|f| f.group == self.group).count();
        self.out.push(FixtureSpec {
            path: format!("{}/{}_{n:03}.{ext}", self.group.dir(), self.prefix),
            group: self.group,
            description,
            body,
        });
    }
}
