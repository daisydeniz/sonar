//! Loop root note (T10.3) on seeded synthetic loops: tom and bass loops at
//! known roots (timing, velocity, octave and passing-note variation drawn
//! from a seeded PRNG), mixed-pitch loops and the combination rule.

use super::root::{combine, onset_frames, Root};
use super::*;
use crate::classify::{KindScores, KindSource};
use crate::decode::ANALYSIS_RATE;
use crate::dsp::Analyzer;
use crate::fixtures::rng::SplitMix64;
use crate::text::Mode;

const SR: u32 = ANALYSIS_RATE;
/// Provisional low-tier bound (T10.4 fits the real one).
const LOW_TIER: f32 = 0.3;

fn hz(midi: f64) -> f64 {
    440.0 * 2f64.powf((midi - 69.0) / 12.0)
}

/// Tom: sine gliding down from 1.3 × f0 (time constant 25 ms) with a
/// second partial at 1.5 × f0 and a short noise click.
fn tom(midi: f64, secs: f64, rng: &mut SplitMix64) -> Vec<f32> {
    let (f0, sr) = (hz(midi), f64::from(SR));
    let glide = 0.025;
    (0..(secs * sr) as usize)
        .map(|i| {
            let t = i as f64 / sr;
            // Phase of f0 · (1 + 0.3 · e^(−t/glide)).
            let ph = std::f64::consts::TAU * f0 * (t + 0.3 * glide * (1.0 - (-t / glide).exp()));
            let body = ph.sin() + 0.25 * (1.5 * ph).sin() * (-t / 0.08).exp();
            let click = 0.3 * rng.next_bipolar() * (-t / 0.004).exp();
            (0.5 * (body * (-t / 0.25).exp() + click)) as f32
        })
        .collect()
}

/// Bass: saw-like tone (6 partials), 5 ms attack, 0.3 s decay.
fn bass(midi: f64, secs: f64, _rng: &mut SplitMix64) -> Vec<f32> {
    let (f0, sr) = (hz(midi), f64::from(SR));
    (0..(secs * sr) as usize)
        .map(|i| {
            let t = i as f64 / sr;
            let s: f64 = (1..=6)
                .map(|k| (std::f64::consts::TAU * f0 * f64::from(k) * t).sin() / f64::from(k))
                .sum();
            (0.4 * s * (t / 0.005).min(1.0) * (-t / 0.3).exp()) as f32
        })
        .collect()
}

type Voice = fn(f64, f64, &mut SplitMix64) -> Vec<f32>;

/// Two bars at `bpm` of eighth-note hits; `pick` gives each hit's MIDI note
/// (or `None` for a rest). Hits get ±8 ms timing and ±25 % velocity jitter;
/// `mono` hits last one step.
fn render_loop(
    voice: Voice,
    mono: bool,
    bpm: f64,
    rng: &mut SplitMix64,
    mut pick: impl FnMut(usize, &mut SplitMix64) -> Option<f64>,
) -> Vec<f32> {
    let step = 60.0 / bpm / 2.0;
    let len = (16.0 * step * f64::from(SR)) as usize;
    let mut out = vec![0.0f32; len];
    for i in 0..16 {
        let Some(m) = pick(i, rng) else { continue };
        let vel = 1.0 + 0.25 * rng.next_bipolar();
        let at = ((i as f64 * step + 0.008 * rng.next_bipolar()).max(0.0) * f64::from(SR)) as usize;
        // Toms ring over the next hits; a bass line is monophonic (a note
        // ends where the next begins).
        let hold = if mono { step } else { 1.0 };
        let hit = voice(m, hold, rng);
        // Linear fade-out: an abrupt cut would be a click (an onset).
        let fade = (hold / 4.0).min(0.05) * f64::from(SR);
        let fade = fade.max(1.0) as usize;
        for (j, v) in hit.iter().enumerate() {
            let g = ((hit.len() - j) as f32 / fade as f32).min(1.0);
            let k = (at + j) % len; // seamless: tails wrap to the start
            out[k] += (vel as f32) * g * v;
        }
    }
    out
}

fn as_loop() -> KindResult {
    KindResult {
        kind: Kind::Loop,
        confidence: 0.9,
        loop_probability: 0.9,
        source: KindSource::Detected,
        hint: None,
        audio_logit: 0.0,
        scores: KindScores::default(),
    }
}

/// Key of `x` analysed as a loop, and its root.
fn analyse(x: &[f32], tr: &mut NoteTracker) -> (Option<KeyEstimate>, Option<Root>) {
    let f = Analyzer::new(SR)
        .expect("analyzer")
        .analyze(x)
        .expect("feats");
    let kn = key_and_note(&as_loop(), &f, x, tr, None).expect("key");
    assert_eq!(kn.note, None);
    (kn.key, loop_root(&f, x, tr).expect("root"))
}

/// 16 pitched loops (8 tom, 8 bass) at seeded roots: the root holds most
/// hits, the rest are octaves, fifths, or passing notes.
#[test]
fn pitched_loops_get_their_root() {
    let mut tr = NoteTracker::new(SR).expect("tracker");
    let mut rng = SplitMix64::for_stream(0x5EED, "loop-root");
    let (mut n, mut ok, mut misses) = (0usize, 0usize, Vec::new());
    for i in 0..16 {
        let is_tom = i < 8;
        let (lo, span) = if is_tom { (40u64, 16) } else { (28, 17) };
        let root = (lo + rng.next_u64() % span) as f64;
        let bpm = 85.0 + (rng.next_u64() % 60) as f64;
        let voice: Voice = if is_tom { tom } else { bass };
        let x = render_loop(voice, !is_tom, bpm, &mut rng, |step, r| {
            let u = r.next_f64();
            match step % 4 {
                0 => Some(root),
                _ if u < 0.2 => None,
                _ if u < 0.65 => Some(root),
                _ if u < 0.8 => Some(root + 12.0),
                _ if u < 0.9 => Some(root + 7.0),
                _ => Some(root + if is_tom { 5.0 } else { 3.0 }),
            }
        });
        let (key, r) = analyse(&x, &mut tr);
        let key = key.expect("key");
        let want = (root as u8) % 12;
        println!(
            "{} root {root} bpm {bpm}: key {} conf {:.2} {:?}, root {r:?}",
            if is_tom { "tom " } else { "bass" },
            key.key,
            key.confidence,
            key.source
        );
        n += 1;
        let hit = key.key.root == want && r.is_some_and(|r| r.pitch_class == want);
        ok += usize::from(hit);
        if !hit {
            misses.push((i, root));
        }
    }
    let acc = ok as f64 / n as f64;
    println!("root accuracy {ok}/{n} = {:.1} %", acc * 100.0);
    assert!(acc >= 0.9, "root accuracy {acc:.3}: {misses:?}");
}

/// Tom loops spread evenly over several pitch classes: no root, and the
/// chroma guess stays in the low tier.
#[test]
fn mixed_pitch_loops_stay_low() {
    let mut tr = NoteTracker::new(SR).expect("tracker");
    let mut rng = SplitMix64::for_stream(0x5EED, "loop-mixed");
    for pitches in [
        [41.0, 44.0, 47.0, 50.0],
        [40.0, 46.0, 49.0, 55.0],
        [43.0, 45.0, 48.0, 51.0],
    ] {
        for (voice, mono) in [(tom as Voice, false), (bass, true)] {
            let x = render_loop(voice, mono, 110.0, &mut rng, |step, _| {
                Some(pitches[step % 4])
            });
            let (key, r) = analyse(&x, &mut tr);
            let key = key.expect("best guess");
            println!(
                "mixed {pitches:?}: key {} conf {:.2} root {r:?}",
                key.key, key.confidence
            );
            assert_eq!(r, None, "{pitches:?}");
            assert!(key.confidence < LOW_TIER, "{pitches:?} {key:?}");
        }
    }
}

#[test]
fn onsets_are_picked_from_envelope_peaks() {
    let mut env = vec![0.0f32; 100];
    for (t, v) in [(10, 5.0), (12, 3.0), (40, 4.0), (41, 4.0), (70, 0.2)] {
        env[t] = v;
    }
    env[90] = f32::NAN;
    // 12 is masked by 10; 41 ties 40 within the gap; 70 is under 10 %.
    assert_eq!(onset_frames(&env), vec![10, 40]);
    assert!(onset_frames(&[]).is_empty());
    assert!(onset_frames(&[0.0; 10]).is_empty());
}

#[test]
fn root_combines_with_the_chroma_key() {
    let root = |pc, c| Root {
        pitch_class: pc,
        share: 0.8,
        confidence: c,
    };
    let am = Key {
        root: 9,
        mode: Some(Mode::Minor),
    };
    let close = |a: f32, b: f32| (a - b).abs() < 1e-6;
    // Same tonic, tonal chroma: the chroma mode, noisy-or confidence.
    let (k, c) = combine(Some(root(9, 0.6)), Some((am, 0.5))).unwrap();
    assert!(k == am && close(c, 0.8), "{k:?} {c}");
    // Same tonic, untonal chroma: mode-less.
    let (k, c) = combine(Some(root(9, 0.6)), Some((am, 0.1))).unwrap();
    assert!(
        k.mode.is_none() && k.root == 9 && close(c, 0.64),
        "{k:?} {c}"
    );
    // Disagree: the stronger wins, discounted.
    let (k, c) = combine(Some(root(2, 0.6)), Some((am, 0.2))).unwrap();
    assert!(
        k.root == 2 && k.mode.is_none() && close(c, 0.48),
        "{k:?} {c}"
    );
    let (k, c) = combine(Some(root(2, 0.3)), Some((am, 0.8))).unwrap();
    assert!(k == am && close(c, 0.56), "{k:?} {c}");
    // One side only.
    assert_eq!(combine(None, Some((am, 0.4))), Some((am, 0.4)));
    let (k, c) = combine(Some(root(4, 0.7)), None).unwrap();
    assert!(k.root == 4 && k.mode.is_none() && close(c, 0.7));
    assert_eq!(combine(None, None), None);
}

/// Release timing of the root pass on a 30 s bass loop: `cargo test
/// --release -p sonar-core key::root_tests::timing -- --ignored --nocapture`.
#[test]
#[ignore = "timing run"]
fn timing_30s_loop() {
    use std::time::Instant;
    let mut rng = SplitMix64::for_stream(1, "timing");
    let bar = render_loop(bass, true, 120.0, &mut rng, |_, _| Some(33.0));
    let x: Vec<f32> = bar.iter().copied().cycle().take(30 * SR as usize).collect();
    let f = Analyzer::new(SR).expect("analyzer").analyze(&x).expect("f");
    let mut tr = NoteTracker::new(SR).expect("tracker");
    let reps = 10;
    let t = Instant::now();
    for _ in 0..reps {
        std::hint::black_box(loop_root(&f, &x, &mut tr).expect("root"));
    }
    let ms = t.elapsed().as_secs_f64() * 1e3 / f64::from(reps);
    println!("30 s loop: root {ms:.2} ms");
}

#[test]
fn silent_and_empty_loops_have_no_root() {
    let mut tr = NoteTracker::new(SR).expect("tracker");
    for x in [vec![0.0f32; 22_050], Vec::new(), vec![f32::NAN; 8000]] {
        let f = Analyzer::new(SR).expect("analyzer").analyze(&x).expect("f");
        assert_eq!(loop_root(&f, &x, &mut tr).expect("root"), None);
    }
}
