//! Unit tests of key and note detection on synthetic signals, plus an
//! ignored timing run.

use super::profiles::{harmonic_expand, key_of_index, key_scores, rotated_correlation};
use super::*;
use crate::classify::{classify_kind, KindScores};
use crate::decode::ANALYSIS_RATE;
use crate::dsp::Analyzer;
use crate::text::{Mode, PriorSource};

const SR: u32 = ANALYSIS_RATE;

fn hz(midi: f64) -> f64 {
    440.0 * 2f64.powf((midi - 69.0) / 12.0)
}

/// Additive tone: `partials[k]` is the amplitude of harmonic `k + 1`;
/// exponential decay `tau` s (`f64::INFINITY` = sustained).
fn tone(midi: f64, partials: &[f64], secs: f64, tau: f64) -> Vec<f32> {
    let f0 = hz(midi);
    let n = (secs * f64::from(SR)) as usize;
    (0..n)
        .map(|i| {
            let t = i as f64 / f64::from(SR);
            let s: f64 = partials
                .iter()
                .enumerate()
                .filter(|(k, _)| f0 * (*k as f64 + 1.0) < 0.45 * f64::from(SR))
                .map(|(k, a)| a * (std::f64::consts::TAU * f0 * (k as f64 + 1.0) * t).sin())
                .sum();
            (0.3 * s * (-t / tau).exp()) as f32
        })
        .collect()
}

fn saw() -> Vec<f64> {
    (1..=8).map(|k| 1.0 / f64::from(k)).collect()
}

/// Chords (MIDI notes) of `secs` each, saw-like stabs, all shifted by
/// `detune` semitones.
fn progression(chords: &[[f64; 4]], secs: f64, detune: f64) -> Vec<f32> {
    let mut out = Vec::new();
    for chord in chords {
        let mut seg = vec![0.0f32; (secs * f64::from(SR)) as usize];
        for &m in chord {
            for (o, v) in seg.iter_mut().zip(tone(m + detune, &saw(), secs, 0.6)) {
                *o += 0.5 * v;
            }
        }
        out.extend(seg);
    }
    out
}

/// Deterministic white noise in `[-amp, amp)`.
fn noise(amp: f32, len: usize) -> Vec<f32> {
    let mut s = 0x9E37_79B9_7F4A_7C15u64;
    (0..len)
        .map(|_| {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            amp * ((s >> 40) as f32 / (1u64 << 23) as f32 - 1.0)
        })
        .collect()
}

/// Kick/808: sine sweeping from `f_start` to `f_end` (time constant
/// `sweep_tau`), amplitude decay `decay_tau`.
fn kick(f_start: f64, f_end: f64, sweep_tau: f64, decay_tau: f64, secs: f64) -> Vec<f32> {
    let sr = f64::from(SR);
    (0..(secs * sr) as usize)
        .map(|i| {
            let t = i as f64 / sr;
            let phase = std::f64::consts::TAU
                * (f_end * t + (f_start - f_end) * sweep_tau * (1.0 - (-t / sweep_tau).exp()));
            (0.8 * phase.sin() * (-t / decay_tau).exp()) as f32
        })
        .collect()
}

fn features(x: &[f32]) -> Features {
    Analyzer::new(SR)
        .expect("analyzer")
        .analyze(x)
        .expect("features")
}

fn key(root: u8, mode: Mode) -> Key {
    Key {
        root,
        mode: Some(mode),
    }
}

fn prior(root: u8, mode: Option<Mode>, explicit: bool) -> KeyPrior {
    KeyPrior {
        key: Key { root, mode },
        explicit,
        source: PriorSource::Filename,
    }
}

/// I–IV–V–I (major) or i–iv–V–i (minor, harmonic-minor V) on `tonic`,
/// bass in octave 2 and a close triad around middle C.
fn cadence(tonic: u8, mode: Mode) -> Vec<[f64; 4]> {
    let t = f64::from(tonic);
    let triad = |root: f64, minor: bool| {
        let r = 57.0 + (root - 57.0).rem_euclid(12.0);
        [
            36.0 + root.rem_euclid(12.0),
            r,
            r + if minor { 3.0 } else { 4.0 },
            r + 7.0,
        ]
    };
    let minor = mode == Mode::Minor;
    vec![
        triad(t, minor),
        triad(t + 5.0, minor),
        triad(t + 7.0, false),
        triad(t, minor),
    ]
}

#[test]
fn profiles_and_scores() {
    assert_eq!(key_of_index(0), key(0, Mode::Major));
    assert_eq!(key_of_index(19), key(9, Mode::Minor));
    let e = harmonic_expand(&[1.0, 0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0.]);
    assert!(
        (e[0] - 1.816).abs() < 1e-4 && (e[7] - 0.36).abs() < 1e-4,
        "{e:?}"
    );
    assert_eq!(rotated_correlation(&[1.0; 12], &profiles::KK_MAJOR, 0), 0.0);
    let mut rot = [0.0f32; 12];
    for (pc, r) in rot.iter_mut().enumerate() {
        *r = profiles::TEMPERLEY_MINOR[(pc + 12 - 5) % 12];
    }
    assert!((rotated_correlation(&rot, &profiles::TEMPERLEY_MINOR, 5) - 1.0).abs() < 1e-5);
    // Symbolic scales: C major and A harmonic minor (tonic triad doubled).
    let scale = |pcs: &[usize], triad: &[usize]| {
        let mut c = [0.0f32; 12];
        pcs.iter().for_each(|&p| c[p] += 1.0);
        triad.iter().for_each(|&p| c[p] += 1.0);
        c
    };
    let best = |c: &[f32; 12]| {
        let s = key_scores(c);
        key_of_index((0..24).fold(0, |b, i| if s[i] > s[b] { i } else { b }))
    };
    assert_eq!(
        best(&scale(&[0, 2, 4, 5, 7, 9, 11], &[0, 4, 7])),
        key(0, Mode::Major)
    );
    assert_eq!(
        best(&scale(&[9, 11, 0, 2, 4, 5, 8], &[9, 0, 4])),
        key(9, Mode::Minor)
    );
    assert_eq!(key_scores(&[0.0; 12]), [0.0; 24]);
}

#[test]
fn cadences_give_their_key_in_tune_and_detuned() {
    for (tonic, mode) in [
        (0, Mode::Major),
        (9, Mode::Minor),
        (6, Mode::Minor),
        (10, Mode::Major),
        (4, Mode::Minor),
    ] {
        for detune in [0.0, 0.35, -0.3] {
            let x = progression(&cadence(tonic, mode), 0.6, detune);
            let tc = tonal_chroma(&features(&x).spectrogram);
            assert!(
                (tc.tuning - detune as f32).abs() < 0.06,
                "tuning {}",
                tc.tuning
            );
            let k = estimate_key(&features(&x), None).expect("key");
            assert_eq!(k.key, key(tonic, mode), "{tonic} {mode:?} {detune}: {k:?}");
            assert_eq!(k.source, KeySource::Detected);
            assert!(k.confidence > 0.5, "{k:?}");
        }
    }
}

#[test]
fn untonal_input_has_no_key_unless_named() {
    let n = SR as usize;
    let single = tone(69.0, &[1.0], 1.0, f64::INFINITY);
    for x in [
        noise(0.5, n),
        vec![0.0; n],
        Vec::new(),
        single,
        vec![f32::NAN; n],
    ] {
        let f = features(&x);
        assert_eq!(estimate_key(&f, None), None);
        let named = estimate_key(&f, Some(prior(9, Some(Mode::Minor), true))).expect("prior");
        assert_eq!(named.key, key(9, Mode::Minor));
        assert_eq!(
            (named.source, named.confidence),
            (KeySource::Filename, NAME_CONF_EXPLICIT)
        );
    }
    let bare = estimate_key(&features(&noise(0.5, n)), Some(prior(0, None, false))).unwrap();
    assert_eq!((bare.key.mode, bare.confidence), (None, NAME_CONF_BARE));
}

#[test]
fn agreeing_prior_raises_confidence_and_wins_close_calls() {
    let f = features(&progression(&cadence(9, Mode::Minor), 0.6, 0.0));
    let plain = estimate_key(&f, None).expect("key");
    let agree = estimate_key(&f, Some(prior(9, None, true))).expect("key");
    assert_eq!(agree.key, plain.key);
    assert!(agree.confidence > plain.confidence);
    // The relative major named in the file: the bonus outweighs a small
    // margin only; a clear minor cadence stays minor.
    let other = estimate_key(&f, Some(prior(0, Some(Mode::Major), true))).expect("key");
    assert_eq!(other.key, key(9, Mode::Minor));
}

#[test]
fn notes_of_sines_and_harmonic_tones_are_exact() {
    let mut tr = NoteTracker::new(SR).expect("tracker");
    for midi in [24u8, 28, 33, 40, 47, 55, 60, 69, 76, 84, 96, 105] {
        let n = tr
            .detect(&tone(f64::from(midi), &[1.0], 1.0, f64::INFINITY))
            .unwrap()
            .unwrap_or_else(|| panic!("no note for {midi}"));
        assert_eq!(n.midi, midi, "{n:?}");
        assert!(n.cents.abs() < 5.0 && n.confidence > 0.9, "{n:?}");
    }
    // Rich low tones and a weak fundamental: no octave errors.
    for (midi, partials) in [
        (28u8, saw()),
        (36, saw()),
        (45, saw()),
        (40, vec![0.3, 1.0, 0.8, 0.5]),
        (52, vec![1.0, 0.0, 0.6, 0.0, 0.4]),
    ] {
        let n = tr
            .detect(&tone(f64::from(midi), &partials, 1.5, 0.4))
            .unwrap()
            .unwrap_or_else(|| panic!("no note for {midi}"));
        assert_eq!(n.midi, midi, "{n:?}");
    }
    let n = tr.detect(&tone(60.3, &saw(), 1.0, 0.5)).unwrap().unwrap();
    assert_eq!(n.midi, 60);
    assert!((n.cents - 30.0).abs() < 3.0, "{n:?}");
    assert_eq!((n.name(), n.pitch_class()), ("C4".to_owned(), 0));
    // 10 ms blip: shorter than a YIN frame.
    let blip = tone(81.0, &[1.0], 0.01, f64::INFINITY);
    assert_eq!(tr.detect(&blip).unwrap().map(|n| n.midi), Some(81));
}

#[test]
fn drums_noise_and_silence_have_no_note_but_808s_do() {
    let mut tr = NoteTracker::new(SR).expect("tracker");
    let n = SR as usize;
    for x in [
        kick(160.0, 48.0, 0.03, 0.15, 0.5),
        kick(120.0, 55.0, 0.03, 0.3, 0.9),
        noise(0.5, n),
        vec![0.0; n],
        Vec::new(),
        vec![0.5],
        vec![f32::NAN; 4000],
    ] {
        assert_eq!(tr.detect(&x).unwrap(), None);
    }
    // 808: brief sweep into a long G1 (49 Hz) sustain.
    let n = tr.detect(&kick(100.0, hz(31.0), 0.015, 0.8, 1.5)).unwrap();
    assert_eq!(n.map(|n| n.midi), Some(31));
}

fn result(kind: Kind, source: KindSource) -> KindResult {
    KindResult {
        kind,
        confidence: 0.9,
        loop_probability: 0.5,
        source,
        hint: None,
        audio_logit: 0.0,
        scores: KindScores::default(),
    }
}

#[test]
fn key_and_note_gating() {
    let mut tr = NoteTracker::new(SR).expect("tracker");
    let pluck = tone(57.0, &saw(), 1.5, 0.35);
    let f = features(&pluck);
    let kind = classify_kind(&f, 1.5, None);
    assert_eq!(kind.kind, Kind::OneShot);
    let kn = key_and_note(&kind, &f, &pluck, &mut tr, None).unwrap();
    assert_eq!(kn.note.map(|n| n.midi), Some(57));
    let k = kn.key.expect("note key");
    assert_eq!(
        (k.key.root, k.key.mode, k.source),
        (9, None, KeySource::Detected)
    );
    // A name on the same root lends its mode; another root does not win.
    let named = key_and_note(
        &kind,
        &f,
        &pluck,
        &mut tr,
        Some(prior(9, Some(Mode::Minor), true)),
    );
    let k = named.unwrap().key.unwrap();
    assert_eq!(k.key, key(9, Mode::Minor));
    assert!(k.confidence >= NAME_CONF_EXPLICIT);
    let other = key_and_note(&kind, &f, &pluck, &mut tr, Some(prior(2, None, true)));
    assert_eq!(other.unwrap().key.unwrap().key.root, 9);
    // Loops get no note; a chord loop gets the chroma key.
    let x = progression(&cadence(7, Mode::Major), 0.5, 0.0);
    let fl = features(&x);
    let kn = key_and_note(
        &result(Kind::Loop, KindSource::Detected),
        &fl,
        &x,
        &mut tr,
        None,
    );
    let kn = kn.unwrap();
    assert_eq!(
        (kn.note, kn.key.map(|k| k.key)),
        (None, Some(key(7, Mode::Major)))
    );
    // Silent fallback: only the name.
    let silent = vec![0.0f32; 4000];
    let fs = features(&silent);
    let fallback = result(Kind::OneShot, KindSource::Fallback);
    let kn = key_and_note(&fallback, &fs, &silent, &mut tr, None).unwrap();
    assert_eq!(kn, KeyNote::default());
    let p = Some(prior(4, Some(Mode::Minor), false));
    let kn = key_and_note(&fallback, &fs, &silent, &mut tr, p).unwrap();
    assert_eq!(kn.key.map(|k| k.source), Some(KeySource::Filename));
}

/// Release timing per 30 s file: `cargo test --release -p sonar-core
/// key::tests::timing -- --ignored --nocapture`.
#[test]
#[ignore = "timing run"]
fn timing_30s() {
    use std::time::Instant;
    let x = progression(&cadence(2, Mode::Major), 2.0, 0.0).repeat(4);
    let f = features(&x[..30 * SR as usize]);
    let pad = tone(50.0, &[1.0, 0.3, 0.1], 30.0, f64::INFINITY);
    let mut tr = NoteTracker::new(SR).expect("tracker");
    let reps = 20;
    let t = Instant::now();
    for _ in 0..reps {
        std::hint::black_box(estimate_key(&f, None));
    }
    let key_ms = t.elapsed().as_secs_f64() * 1e3 / f64::from(reps);
    let t = Instant::now();
    for _ in 0..reps {
        std::hint::black_box(tr.detect(&pad).unwrap());
    }
    let note_ms = t.elapsed().as_secs_f64() * 1e3 / f64::from(reps);
    println!("30 s window: key {key_ms:.2} ms, note {note_ms:.2} ms");
}
