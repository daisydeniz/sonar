//! Music theory helpers for fixtures: pitch spelling, MIDI → Hz, keys,
//! cadential chord progressions and the chord-loop renderer.

use super::manifest::{ChordTruth, KeyTruth, Mode};
use super::synth::{self, additive, attack_decay, frames_for, mix_into, Audio};

/// Canonical pitch-class spelling used for every note, key and chord name in
/// the manifest (index = pitch class, C = 0): sharps for C#/F#, flats for
/// Eb/Ab/Bb — the most common spelling in sample-library file names.
pub const PITCH_CLASS_NAMES: [&str; 12] = [
    "C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B",
];

/// MIDI note → frequency, 12-TET with A4 (MIDI 69) = 440 Hz:
/// `f = 440 · 2^((m − 69) / 12)`.
pub fn midi_to_hz(midi: f64) -> f64 {
    440.0 * 2f64.powf((midi - 69.0) / 12.0)
}

/// Scientific pitch notation (middle C = C4 = MIDI 60), canonical spelling.
pub fn note_name(midi: u8) -> String {
    let octave = i32::from(midi / 12) - 1;
    format!("{}{}", pc_name(midi), octave)
}

fn pc_name(pc: u8) -> &'static str {
    PITCH_CLASS_NAMES[usize::from(pc % 12)]
}

/// A key: tonic pitch class + mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Key {
    /// Tonic pitch class, 0 = C.
    pub tonic_pc: u8,
    /// Major or minor.
    pub mode: Mode,
}

impl Key {
    /// Major key on `tonic_pc`.
    pub const fn major(tonic_pc: u8) -> Self {
        Self {
            tonic_pc,
            mode: Mode::Major,
        }
    }

    /// Minor key on `tonic_pc`.
    pub const fn minor(tonic_pc: u8) -> Self {
        Self {
            tonic_pc,
            mode: Mode::Minor,
        }
    }

    /// Manifest representation.
    pub fn truth(&self) -> KeyTruth {
        KeyTruth {
            tonic: pc_name(self.tonic_pc).to_owned(),
            tonic_pc: self.tonic_pc % 12,
            mode: self.mode,
        }
    }

    /// Human-readable name, e.g. `"F# minor"`.
    pub fn name(&self) -> String {
        let mode = match self.mode {
            Mode::Major => "major",
            Mode::Minor => "minor",
        };
        format!("{} {mode}", pc_name(self.tonic_pc))
    }
}

/// A triad within a progression.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chord {
    /// Root pitch class.
    pub root_pc: u8,
    /// Triad quality.
    pub quality: Mode,
    /// Roman numeral in the key.
    pub roman: &'static str,
}

impl Chord {
    /// Symbol: root + `"m"` for minor.
    pub fn symbol(&self) -> String {
        let suffix = if self.quality == Mode::Minor { "m" } else { "" };
        format!("{}{suffix}", pc_name(self.root_pc))
    }

    /// MIDI notes: bass root in octave 2 (C2..B2) plus a close-position
    /// triad whose root lies in A3..G#4 (MIDI 57..68).
    pub fn voicing(&self) -> [u8; 4] {
        let pc = self.root_pc % 12;
        let bass = 36 + pc;
        let root = 57 + (pc + 3) % 12;
        let third = root + if self.quality == Mode::Major { 4 } else { 3 };
        [bass, root, third, root + 7]
    }
}

/// Cadential progression: major I–IV–V–I, minor i–iv–V–i (harmonic-minor
/// dominant, so the leading tone disambiguates the relative major).
pub fn progression(key: Key) -> [Chord; 4] {
    let t = key.tonic_pc % 12;
    let c = |offset: u8, quality, roman| Chord {
        root_pc: (t + offset) % 12,
        quality,
        roman,
    };
    match key.mode {
        Mode::Major => [
            c(0, Mode::Major, "I"),
            c(5, Mode::Major, "IV"),
            c(7, Mode::Major, "V"),
            c(0, Mode::Major, "I"),
        ],
        Mode::Minor => [
            c(0, Mode::Minor, "i"),
            c(5, Mode::Minor, "iv"),
            c(7, Mode::Major, "V"),
            c(0, Mode::Minor, "i"),
        ],
    }
}

/// Beats each chord lasts in [`chord_loop`].
pub const BEATS_PER_CHORD: u32 = 2;

/// Renders the key's progression as a seamless loop of `bars` 4/4 bars,
/// one chord per [`BEATS_PER_CHORD`] beats (repeating the progression),
/// re-struck on every beat with a decaying saw-like tone (8 partials, `1/k`)
/// so chroma and onsets are both clear. Returns audio and chord truth.
pub fn chord_loop(
    sample_rate: u32,
    channels: u16,
    key: Key,
    bpm: f64,
    bars: u32,
) -> (Audio, Vec<ChordTruth>) {
    let beat_s = 60.0 / bpm;
    let beats = bars * 4;
    let total = frames_for(f64::from(beats) * beat_s, sample_rate);
    let mut mono = vec![0.0f32; total];
    let partials = synth::saw_partials(8);
    let prog = progression(key);
    let stab_frames = frames_for(beat_s, sample_rate);

    let mut chords = Vec::new();
    for (slot, start_beat) in (0..beats).step_by(BEATS_PER_CHORD as usize).enumerate() {
        let chord = prog[slot % prog.len()];
        // One stab per chord, mixed on each of its beats.
        let mut stab = vec![0.0f32; stab_frames];
        for (i, note) in chord.voicing().into_iter().enumerate() {
            let gain = if i == 0 { 0.8 } else { 0.5 };
            let tone = additive(
                stab_frames,
                sample_rate,
                midi_to_hz(f64::from(note)),
                &partials,
                |t| attack_decay(t, 0.005, beat_s * 0.45),
            );
            mix_into(&mut stab, &tone, 0, gain, false);
        }
        for b in start_beat..start_beat + BEATS_PER_CHORD {
            let at = frames_for(f64::from(b) * beat_s, sample_rate);
            mix_into(&mut mono, &stab, at, 1.0, true);
        }
        chords.push(ChordTruth {
            symbol: chord.symbol(),
            root: pc_name(chord.root_pc).to_owned(),
            quality: chord.quality,
            roman: chord.roman.to_owned(),
            start_s: round6(f64::from(start_beat) * beat_s),
            duration_s: round6(f64::from(BEATS_PER_CHORD) * beat_s),
        });
    }
    (Audio::from_mono(mono, channels), chords)
}

/// Rounds to 6 decimals (µs) for stable, readable manifest values.
pub fn round6(x: f64) -> f64 {
    (x * 1e6).round() / 1e6
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn midi_to_hz_reference_points() {
        assert_eq!(midi_to_hz(69.0), 440.0);
        assert_eq!(midi_to_hz(57.0), 220.0);
        assert!((midi_to_hz(60.0) - 261.625_565).abs() < 1e-5);
        assert!((midi_to_hz(33.0) - 55.0).abs() < 1e-9);
    }

    #[test]
    fn note_names_use_canonical_spelling() {
        assert_eq!(note_name(60), "C4");
        assert_eq!(note_name(69), "A4");
        assert_eq!(note_name(33), "A1");
        assert_eq!(note_name(96), "C7");
        assert_eq!(note_name(70), "Bb4");
        assert_eq!(note_name(61), "C#4");
        assert_eq!(note_name(0), "C-1");
    }

    #[test]
    fn progressions_and_symbols() {
        let p = progression(Key::minor(6)); // F# minor
        let syms: Vec<String> = p.iter().map(Chord::symbol).collect();
        assert_eq!(syms, ["F#m", "Bm", "C#", "F#m"]);
        let p = progression(Key::major(10)); // Bb major
        let syms: Vec<String> = p.iter().map(Chord::symbol).collect();
        assert_eq!(syms, ["Bb", "Eb", "F", "Bb"]);
        assert_eq!(Key::minor(6).name(), "F# minor");
        assert_eq!(Key::major(3).truth().tonic, "Eb");
    }

    #[test]
    fn voicings_contain_the_triad() {
        let c = Chord {
            root_pc: 9,
            quality: Mode::Minor,
            roman: "i",
        };
        assert_eq!(c.voicing(), [45, 57, 60, 64]); // A2 | A3 C4 E4
        let c = Chord {
            root_pc: 0,
            quality: Mode::Major,
            roman: "I",
        };
        assert_eq!(c.voicing(), [36, 60, 64, 67]);
    }

    #[test]
    fn catalog_progressions_stay_in_key() {
        use crate::fixtures::catalog::{MAJOR_KEYS, MINOR_KEYS};
        const MAJOR: [u8; 7] = [0, 2, 4, 5, 7, 9, 11];
        const HARMONIC_MINOR: [u8; 7] = [0, 2, 3, 5, 7, 8, 11];
        let keys = MAJOR_KEYS.iter().map(|&pc| (Key::major(pc), MAJOR)).chain(
            MINOR_KEYS
                .iter()
                .map(|&pc| (Key::minor(pc), HARMONIC_MINOR)),
        );
        for (key, scale) in keys {
            let prog = progression(key);
            assert_eq!(prog[0].root_pc, key.tonic_pc, "{}", key.name());
            for chord in prog {
                for note in chord.voicing() {
                    let degree = (note % 12 + 12 - key.tonic_pc) % 12;
                    assert!(
                        scale.contains(&degree),
                        "{}: {}",
                        key.name(),
                        chord.symbol()
                    );
                }
            }
        }
    }

    #[test]
    fn chord_loop_length_and_truth() {
        let (audio, chords) = chord_loop(44_100, 2, Key::major(7), 120.0, 2);
        assert_eq!(audio.frames(), 176_400); // 8 beats at 0.5 s
        assert_eq!(audio.channels.len(), 2);
        assert_eq!(chords.len(), 4);
        assert_eq!(chords[2].symbol, "D");
        assert_eq!(chords[2].start_s, 2.0);
        assert_eq!(chords[3].duration_s, 1.0);
        assert!(audio.peak() > 0.1);
    }
}
