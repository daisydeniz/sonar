//! The fixture catalog: every file `sonar fixtures` writes, as data.
//!
//! File names are neutral (`<group>/<prefix>_NNN.<ext>`); truth lives only
//! in the manifest. Formats rotate through [`SPREAD`] so every group
//! exercises several rates/depths/channel counts. Non-WAV containers are
//! listed in [`containers`](super::containers).

use super::containers::containers;
use super::corrupt::CorruptKind;
use super::drums::Pattern;
use super::manifest::{Group, Kind, NoiseColor};
use super::music::Key;
use super::oneshot::{DrumHit, HatParams, KickParams, SnareParams};
use super::spec::{Body, Builder, Content, FixtureSpec};
use super::wav::{SampleFormat, WavSpec};

use NoiseColor::{Pink, White};
use SampleFormat::{Float32, Int16, Int24};

/// Formats rotated through the non-matrix groups.
pub const SPREAD: [WavSpec; 8] = [
    WavSpec::new(44_100, 2, Int16),
    WavSpec::new(48_000, 2, Int24),
    WavSpec::new(44_100, 1, Int16),
    WavSpec::new(96_000, 2, Float32),
    WavSpec::new(48_000, 1, Int16),
    WavSpec::new(44_100, 2, Int24),
    WavSpec::new(48_000, 2, Float32),
    WavSpec::new(96_000, 1, Int24),
];

/// MIDI notes of the tonal group (A1..C7, naturals and accidentals).
pub const TONAL_NOTES: [u8; 22] = [
    33, 36, 40, 43, 46, 49, 52, 55, 58, 60, 61, 64, 66, 69, 70, 74, 78, 81, 85, 88, 91, 96,
];

/// Chord-loop tonics (pitch class) of the major keys: C G D E Bb Eb Ab F#.
pub const MAJOR_KEYS: [u8; 8] = [0, 7, 2, 4, 10, 3, 8, 6];
/// Chord-loop tonics of the minor keys: A E B F# C# F G C.
pub const MINOR_KEYS: [u8; 8] = [9, 4, 11, 6, 1, 5, 7, 0];

/// Kick one-shots: `(f_start Hz, f_end Hz, decay_tau s, secs, description)`.
const KICKS: [(f64, f64, f64, f64, &str); 4] = [
    (
        160.0,
        48.0,
        0.15,
        0.5,
        "kick: 160→48 Hz sweep, 150 ms decay",
    ),
    (
        120.0,
        55.0,
        0.30,
        0.9,
        "kick: long boomy 120→55 Hz, 300 ms decay",
    ),
    (
        220.0,
        60.0,
        0.07,
        0.3,
        "kick: short punchy 220→60 Hz, 70 ms decay",
    ),
    (140.0, 40.0, 0.45, 1.2, "kick: sub 140→40 Hz, 450 ms decay"),
];

/// Snare one-shots: `(body Hz, noise_tau s, noise high-pass Hz, description)`.
const SNARES: [(f64, f64, f64, &str); 3] = [
    (180.0, 0.10, 1_500.0, "snare: 180 Hz body + noise"),
    (
        220.0,
        0.16,
        2_500.0,
        "snare: bright 220 Hz body, longer noise",
    ),
    (160.0, 0.06, 1_000.0, "snare: tight 160 Hz body"),
];

/// Hat one-shots: `(open?, high-pass Hz, decay_tau s, secs, description)`.
const HATS: [(bool, f64, f64, f64, &str); 5] = [
    (
        false,
        7_000.0,
        0.02,
        0.08,
        "closed hat: HP noise, 20 ms decay",
    ),
    (false, 9_000.0, 0.012, 0.05, "closed hat: very short tick"),
    (
        false,
        5_500.0,
        0.035,
        0.12,
        "closed hat: darker, 35 ms decay",
    ),
    (true, 6_000.0, 0.15, 0.6, "open hat: HP noise, 150 ms decay"),
    (true, 7_500.0, 0.25, 0.9, "open hat: long, 250 ms decay"),
];

/// Optional noise burst window `(start s, length s)`.
type Burst = Option<(f64, f64)>;

/// Noise: `(color, secs, burst, description)`.
const NOISES: [(NoiseColor, f64, Burst, &str); 4] = [
    (White, 1.0, None, "steady white noise, 1 s"),
    (Pink, 2.0, None, "steady pink noise (Kellet filter), 2 s"),
    (
        White,
        1.0,
        Some((0.3, 0.2)),
        "white burst 0.3–0.5 s in 1 s of silence",
    ),
    (
        Pink,
        1.5,
        Some((0.5, 0.5)),
        "pink burst 0.5–1.0 s in 1.5 s of silence",
    ),
];

/// Drum loops: `(bpm, bars, pattern)`, tempi spanning 70–175 incl. 122.5.
pub const DRUM_LOOPS: [(f64, u32, Pattern); 13] = [
    (70.0, 2, Pattern::Backbeat),
    (85.0, 2, Pattern::Breakbeat),
    (90.0, 2, Pattern::Backbeat),
    (100.0, 4, Pattern::Backbeat),
    (110.0, 4, Pattern::Breakbeat),
    (120.0, 4, Pattern::FourOnFloor),
    (122.5, 4, Pattern::FourOnFloor),
    (128.0, 4, Pattern::FourOnFloor),
    (140.0, 4, Pattern::HalfTime),
    (150.0, 4, Pattern::Backbeat),
    (160.0, 4, Pattern::Breakbeat),
    (174.0, 4, Pattern::Breakbeat),
    (175.0, 4, Pattern::Breakbeat),
];

/// Click loops: `(bpm, bars, beats_per_bar)`.
pub const CLICK_LOOPS: [(f64, u32, u32); 4] =
    [(72.0, 2, 4), (95.0, 4, 3), (133.0, 4, 4), (168.0, 4, 4)];

/// Length of the long fixture (> 5 min).
pub const LONG_SECS: f64 = 310.0;

fn spread(i: usize) -> WavSpec {
    SPREAD[i % SPREAD.len()]
}

fn sine(midi: u8, secs: f64, kind: Kind) -> Content {
    Content::Sine { midi, secs, kind }
}

/// The full catalog in a fixed order. `include_long` adds the > 5 min file.
pub fn catalog(include_long: bool) -> Vec<FixtureSpec> {
    let mut b = Builder::new();
    tonal(&mut b);
    loops(&mut b);
    oneshots(&mut b);
    noise(&mut b);
    edge(&mut b, include_long);
    formats(&mut b);
    containers(&mut b);
    b.group(Group::Corrupt, "bad");
    for k in CorruptKind::ALL {
        b.push(Body::Corrupt(k), k.description().to_owned());
    }
    b.out
}

fn tonal(b: &mut Builder) {
    b.group(Group::Tonal, "tone");
    for (i, &midi) in TONAL_NOTES.iter().enumerate() {
        let (content, peak, desc) = if i % 2 == 0 {
            (sine(midi, 2.0, Kind::Tone), -6.0, "sustained sine, 2 s")
        } else {
            let desc = "plucked additive tone, 1.5 s decay";
            (Content::Pluck { midi, secs: 1.5 }, -3.0, desc)
        };
        b.add(spread(i), content, Some(peak), desc);
    }
    b.group(Group::Chords, "prog");
    let bpms = [90.0, 100.0, 110.0, 120.0, 96.0, 105.0, 115.0, 124.0];
    let pairs = MAJOR_KEYS.iter().zip(MINOR_KEYS);
    let keys = pairs.flat_map(|(&maj, min)| [Key::major(maj), Key::minor(min)]);
    for (i, key) in keys.enumerate() {
        let bpm = bpms[i % bpms.len()];
        let name = key.name();
        let desc = format!("{name} progression, saw stabs on every beat, {bpm} BPM, 2 bars");
        let content = Content::Chords { key, bpm, bars: 2 };
        b.add(spread(i + 3), content, Some(-1.0), desc);
    }
}

fn loops(b: &mut Builder) {
    b.group(Group::Loops, "loop");
    for (i, (bpm, bars, pattern)) in DRUM_LOOPS.into_iter().enumerate() {
        let desc = format!("{pattern:?} drum loop, {bpm} BPM, {bars} bars of 4/4");
        let content = Content::DrumLoop { bpm, bars, pattern };
        b.add(spread(i + 1), content, Some(-1.0), desc);
    }
    for (i, (bpm, bars, bpb)) in CLICK_LOOPS.into_iter().enumerate() {
        let desc = format!("metronome click, {bpm} BPM, {bars} bars of {bpb}/4, accent on 1");
        let content = Content::ClickLoop {
            bpm,
            bars,
            beats_per_bar: bpb,
        };
        b.add(spread(i + 5), content, Some(-3.0), desc);
    }
}

fn oneshots(b: &mut Builder) {
    b.group(Group::Oneshots, "hit");
    let mut hits = Vec::new();
    for (f_start, f_end, decay_tau, secs, desc) in KICKS {
        let p = KickParams {
            f_start,
            f_end,
            sweep_tau: 0.035,
            decay_tau,
            secs,
        };
        hits.push((DrumHit::Kick(p), desc));
    }
    for (body_hz, noise_tau, noise_hp_hz, desc) in SNARES {
        let (body_tau, secs) = (0.07, 0.35);
        let p = SnareParams {
            body_hz,
            body_tau,
            noise_tau,
            noise_hp_hz,
            secs,
        };
        hits.push((DrumHit::Snare(p), desc));
    }
    for (open, hp_hz, decay_tau, secs, desc) in HATS {
        let p = HatParams {
            hp_hz,
            decay_tau,
            secs,
        };
        let hit = if open {
            DrumHit::HatOpen(p)
        } else {
            DrumHit::HatClosed(p)
        };
        hits.push((hit, desc));
    }
    for (i, (hit, desc)) in hits.into_iter().enumerate() {
        b.add(spread(i + 2), Content::Hit(hit), Some(-1.0), desc);
    }
}

fn noise(b: &mut Builder) {
    b.group(Group::Noise, "noise");
    for (i, (color, secs, burst, desc)) in NOISES.into_iter().enumerate() {
        let content = Content::Noise { color, secs, burst };
        b.add(spread(i + 3), content, Some(-6.0), desc);
    }
}

fn edge(b: &mut Builder, include_long: bool) {
    b.group(Group::Edge, "edge");
    let m16 = WavSpec::new(44_100, 1, Int16);
    let silence = |frames| Content::Silence { frames };
    b.add(m16, silence(44_100), None, "digital silence, 1 s");
    let s = WavSpec::new(48_000, 2, Float32);
    b.add(s, silence(96_000), None, "digital silence, 2 s float");
    b.add(m16, silence(0), None, "valid header, zero frames");
    let s = WavSpec::new(96_000, 1, Int24);
    b.add(s, Content::Impulse, None, "single-frame impulse");
    let d = "very short 5 ms A5 sine burst";
    b.add(m16, sine(81, 0.005, Kind::OneShot), Some(-3.0), d);
    let (s, d) = (
        WavSpec::new(48_000, 2, Int24),
        "very short 15 ms A5 sine burst",
    );
    b.add(s, sine(81, 0.015, Kind::OneShot), Some(-3.0), d);
    let (s, d) = (
        WavSpec::new(44_100, 2, Int24),
        "very quiet A4 sine, -60 dBFS peak",
    );
    b.add(s, sine(69, 1.0, Kind::Tone), Some(-60.0), d);
    let (midi, secs, drive_db) = (57, 1.0, 6.0);
    let clipped = Content::ClippedSine {
        midi,
        secs,
        drive_db,
    };
    b.add(
        m16,
        clipped,
        None,
        "A3 sine driven +6 dB into hard clipping",
    );
    if include_long {
        let d = format!("long D3 pad with slow tremolo, {LONG_SECS} s (> 5 min)");
        let pad = Content::Pad {
            midi: 50,
            secs: LONG_SECS,
        };
        b.add(m16, pad, Some(-18.0), d);
    }
}

fn formats(b: &mut Builder) {
    b.group(Group::Formats, "fmt");
    let a4 = || sine(69, 1.0, Kind::Tone);
    for fmt in [Int16, Int24, Float32] {
        for rate in [44_100, 48_000, 96_000] {
            for ch in [1u16, 2] {
                let d = format!("A4 sine 1 s, {fmt:?}, {rate} Hz, {ch} ch");
                b.add(WavSpec::new(rate, ch, fmt), a4(), Some(-6.0), d);
            }
        }
    }
    for fmt in [(48_000, 2, Int24), (96_000, 1, Float32)] {
        let spec = WavSpec::new(fmt.0, fmt.1, fmt.2).with_extensible();
        let d = format!("A4 sine 1 s, WAVE_FORMAT_EXTENSIBLE {:?}", fmt.2);
        b.add(spec, a4(), Some(-6.0), d);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn paths_are_unique_and_in_their_group_dir() {
        let cat = catalog(true);
        let paths: BTreeSet<&str> = cat.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths.len(), cat.len());
        for f in &cat {
            assert!(
                f.path.starts_with(&format!("{}/", f.group.dir())),
                "{}",
                f.path
            );
            let wav = f.path.ends_with(".wav");
            assert_eq!(wav, f.group != Group::Containers, "{}", f.path);
        }
    }

    #[test]
    fn long_file_is_optional() {
        assert_eq!(catalog(true).len(), catalog(false).len() + 1);
    }

    #[test]
    fn format_matrix_is_complete() {
        let specs: BTreeSet<(u32, u16, u16)> = catalog(false)
            .iter()
            .filter(|f| f.group == Group::Formats)
            .filter_map(|f| match &f.body {
                Body::Audio { spec, store, .. } if !spec.extensible => {
                    assert_eq!(*store, crate::fixtures::spec::Store::Wav);
                    Some((spec.sample_rate, spec.channels, spec.sample_format.bits()))
                }
                _ => None,
            })
            .collect();
        assert_eq!(specs.len(), 18);
    }
}
