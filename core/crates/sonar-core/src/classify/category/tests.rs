//! Tests of the category classifier: taxonomy table, name hints, fusion
//! with synthetic evidence, calibration math, DSP cues on fixtures.

use std::collections::HashSet;
use std::path::Path;

use super::calibrate::{ece, fit_temperature, nll, softmax};
use super::dsp::{dsp_scores, PROFILES};
use super::*;
use crate::classify::Kind;
use crate::dsp::aspects::{
    amplitude, AspectVectors, AMPLITUDE_DIM, PITCH_DIM, SPECTRUM_DIM, TIMBRE_DIM,
};

#[test]
fn taxonomy_rows_are_complete_ordered_and_unique() {
    let mut ids = HashSet::new();
    let mut words = HashSet::new();
    for (i, info) in TAXONOMY.iter().enumerate() {
        assert_eq!(info.category, Category::ALL[i], "{}", info.id);
        assert_eq!(info.category.index(), i);
        assert!(ids.insert(info.id), "duplicate id {}", info.id);
        assert!(!info.prompts.is_empty(), "{} has no prompt", info.id);
        assert!(!info.words.is_empty(), "{} has no name word", info.id);
        for w in info.words.iter().chain(info.weak_words) {
            assert_eq!(*w, w.to_lowercase(), "{w} must be folded");
            assert!(words.insert(*w), "word {w} names two categories");
        }
        assert_eq!(Category::parse(info.id), Some(info.category));
        assert_eq!(info.category.to_string(), info.id);
        assert_eq!(PROFILES[i].category, info.category, "profile order");
    }
    assert_eq!(Category::parse("Hi-Hats"), Some(Category::Hat));
    assert_eq!(Category::parse(" Percussion "), Some(Category::Perc));
    assert_eq!(Category::parse("banjo"), None);
    assert_eq!(Category::parse(""), None);
}

/// `docs/categories.md` is the human-readable mirror of [`TAXONOMY`]: every
/// id, prompt and name word must appear there.
#[test]
fn docs_mirror_the_taxonomy() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../docs/categories.md");
    let doc = std::fs::read_to_string(&path).expect("docs/categories.md");
    for info in &TAXONOMY {
        assert!(doc.contains(&format!("`{}`", info.id)), "id {}", info.id);
        for p in info.prompts {
            assert!(doc.contains(p), "{}: prompt {p:?} missing", info.id);
        }
        for w in info.words.iter().chain(info.weak_words) {
            assert!(doc.contains(&format!("`{w}`")), "{}: word {w:?}", info.id);
        }
    }
}

#[test]
fn name_hints_follow_the_taxonomy_words() {
    let cases: &[(&str, Option<Category>)] = &[
        ("Drums/Kicks/Kick_01.wav", Some(Category::Kick)),
        ("Pack/808 Kick.wav", Some(Category::Kick)),
        ("Pack/Bass Drum 01.wav", Some(Category::Kick)),
        ("Pack/808 One Shots/Boom C.wav", Some(Category::Fx)),
        ("Pack/808 One Shots/Long C.wav", Some(Category::Bass)),
        ("Loops/Snare Fills 125 Bpms/x 3.wav", Some(Category::Snare)),
        ("Loops/Fills/Fill 3.wav", Some(Category::Drums)),
        ("Loops/Top Loops 125 Bpms/Top 1.wav", Some(Category::Drums)),
        ("HiHat_Open_01.wav", Some(Category::Hat)),
        ("Pack/Piano Chords.wav", Some(Category::Keys)),
        ("Pack/Synth Chords.wav", Some(Category::Synth)),
        ("Pack/Chords/A.wav", Some(Category::Synth)),
        (
            "EMP - FUTURE BASS One Shots/Snares Future Bass/Snare 1.wav",
            Some(Category::Snare),
        ),
        ("Drum & Bass Pack/Bass/Reese 1.wav", Some(Category::Bass)),
        ("Drum & Bass Pack/x.wav", None),
        (
            "Pack/Atmospheres Loops 125 Bpms/A 1.wav",
            Some(Category::Texture),
        ),
        ("Effects/Fx/Uplifter 3.wav", Some(Category::Fx)),
        ("Vox_Chop_Am.wav", Some(Category::Vocal)),
        // Two categories in one segment: no hint from it, the folder decides.
        ("Synth & Plucks Loops/Bass Pluck.wav", None),
        ("Drums/Kick Snare 01.wav", Some(Category::Drums)),
        ("misc/sample_17.wav", None),
    ];
    for (path, want) in cases {
        assert_eq!(category_hint(Path::new(path)), *want, "{path}");
    }
}

/// Cues of a mid-range, unpitched, short one-shot.
fn cues() -> DspCues {
    DspCues {
        p_loop: 0.05,
        log2_dur: -1.0,
        centroid: 1.8,
        rolloff: 1.6,
        flatness: -11.0,
        low: 0.5,
        high: -4.5,
        tonal: 0.02,
        attack: -2.6,
        decay: -0.95,
        effective: -1.25,
        ..DspCues::default()
    }
}

fn clap_for(c: Category, margin: f32) -> ClapScores {
    let mut s = [0.1f32; N_CATEGORIES];
    s[c.index()] += margin;
    s
}

#[test]
fn dsp_profiles_recognise_their_own_means() {
    for prof in &PROFILES {
        let [cen, roll, flat, low, high, tonal] = prof.spectral.map(|(m, _)| m);
        let [attack, decay, effective, log2_dur] = prof.envelope.map(|(m, _)| m);
        let c = DspCues {
            p_loop: if prof.p_loop > 0.5 { 0.95 } else { 0.05 },
            centroid: cen,
            rolloff: roll,
            flatness: flat,
            low,
            high,
            tonal,
            f0: prof.f0.0,
            attack,
            decay,
            effective,
            log2_dur,
            ..DspCues::default()
        };
        let s = dsp_scores(&c);
        assert!(s.iter().all(|v| v.is_finite() && *v <= 0.0));
        // Its own class scores within a few nats of the best (wide classes
        // like fx can be beaten by a tighter neighbour at their own mean).
        assert!(
            s[prof.category.index()] > -6.0,
            "{:?}: {s:?}",
            prof.category
        );
    }
}

#[test]
fn silent_or_missing_evidence_gives_the_name_or_nothing() {
    let silent = DspCues {
        silent: true,
        ..cues()
    };
    let r = classify_category(&silent, None, None);
    assert_eq!(
        (r.category, r.source, r.confidence),
        (None, CategorySource::Fallback, 0.0)
    );
    // CLAP of a silent clip is ignored too.
    let r = classify_category(&silent, Some(&clap_for(Category::Kick, 0.5)), None);
    assert_eq!(r.category, None);
    let r = classify_category(&silent, None, Some(Category::Pad));
    assert_eq!(
        (r.category, r.source),
        (Some(Category::Pad), CategorySource::Filename)
    );
    assert!(r.confidence > 0.5 && r.confidence < 1.0, "{}", r.confidence);
    // No aspect vectors (DSP never ran): CLAP alone decides.
    let missing = DspCues {
        missing: true,
        ..DspCues::default()
    };
    assert_eq!(classify_category(&missing, None, None).category, None);
    let r = classify_category(&missing, Some(&clap_for(Category::Fx, 0.4)), None);
    assert_eq!(
        (r.category, r.source, r.clap),
        (Some(Category::Fx), CategorySource::Detected, true)
    );
}

#[test]
fn clap_evidence_decides_and_the_name_only_tips_close_calls() {
    let c = cues();
    // DSP alone: a snare-like one-shot.
    let dsp = classify_category(&c, None, None);
    assert_eq!(dsp.category, Some(Category::Snare), "{:?}", dsp.top3());
    assert_eq!(dsp.source, CategorySource::Detected);
    // A clear CLAP vote for clap overrides the DSP lean.
    let strong = clap_for(Category::Clap, 0.6);
    let r = classify_category(&c, Some(&strong), None);
    assert_eq!(r.category, Some(Category::Clap));
    assert!(r.confidence > 0.9, "{}", r.confidence);
    // A name contradicting clear audio does not win…
    let r = classify_category(&c, Some(&strong), Some(Category::Bass));
    assert_eq!(
        (r.category, r.source, r.hint),
        (
            Some(Category::Clap),
            CategorySource::Detected,
            Some(Category::Bass)
        )
    );
    // …but decides between two close candidates.
    let mut close = clap_for(Category::Clap, 0.0);
    close[Category::Snare.index()] = 0.3;
    close[Category::Clap.index()] = 0.3;
    let r = classify_category(&c, Some(&close), Some(Category::Clap));
    assert_eq!(r.category, Some(Category::Clap));
    let audio = classify_category(&c, Some(&close), None).category;
    let expect = if audio == Some(Category::Clap) {
        CategorySource::Detected
    } else {
        CategorySource::Filename
    };
    assert_eq!(r.source, expect);
    let top = r.top3();
    assert_eq!(top[0].0, Category::Clap);
    assert!(top[0].1 >= top[1].1 && top[1].1 >= top[2].1);
}

#[test]
fn outputs_stay_finite_for_hostile_cues() {
    let bad = DspCues {
        centroid: f32::MAX,
        flatness: f32::MIN,
        f0: 1e30,
        tonal: 1.0,
        ..cues()
    };
    let clap = [f32::NAN; N_CATEGORIES];
    for r in [
        classify_category(&bad, None, None),
        classify_category(&bad, Some(&clap), Some(Category::Kick)),
    ] {
        assert!(r.confidence.is_finite() && (0.0..=1.0).contains(&r.confidence));
        assert!(r.logits.iter().all(|v| v.is_finite()));
        assert!(r.category.is_some());
    }
}

#[test]
fn softmax_and_temperature_scaling() {
    let mut p = [0.0f32; 3];
    softmax(&[1.0, 2.0, 3.0], 1.0, &mut p);
    assert!((p.iter().sum::<f32>() - 1.0).abs() < 1e-6);
    let e = [1f32.exp(), 2f32.exp(), 3f32.exp()];
    assert!((p[2] - e[2] / (e[0] + e[1] + e[2])).abs() < 1e-6);
    // Higher temperature flattens, an invalid one counts as 1.
    let mut flat = [0.0f32; 3];
    softmax(&[1.0, 2.0, 3.0], 10.0, &mut flat);
    assert!(flat[2] < p[2] && flat[0] > p[0]);
    let mut same = [0.0f32; 3];
    softmax(&[1.0, 2.0, 3.0], f32::NAN, &mut same);
    assert_eq!(same, p);
    softmax(&[1.0, f32::NAN, 3.0], 1.0, &mut same);
    assert_eq!(same, [1.0 / 3.0; 3]);
}

/// Labels drawn exactly from `softmax(z / 2)`: the fitted temperature is 2.
#[test]
fn fitted_temperature_recovers_the_generating_one() {
    let z = [4.0f32, 0.0];
    let mut p = [0.0f32; 2];
    softmax(&z, 2.0, &mut p);
    let n0 = (p[0] * 1000.0).round() as usize;
    let samples: Vec<(&[f32], usize)> = (0..1000).map(|i| (&z[..], usize::from(i >= n0))).collect();
    let t = fit_temperature(&samples);
    assert!((t - 2.0).abs() < 0.02, "{t}");
    assert!(nll(&samples, t) <= nll(&samples, 1.0));
    assert_eq!(fit_temperature(&[]), 1.0);
}

/// Hand-computed: two bins. Bin 0.9: 4 files, 3 right (gap 0.15); bin
/// 0.5: 2 files, 2 right (gap 0.5). ECE = 4/6·0.15 + 2/6·0.5 = 0.2667.
#[test]
fn ece_matches_hand_computation() {
    let s = [
        (0.9, true),
        (0.9, true),
        (0.9, true),
        (0.9, false),
        (0.5, true),
        (0.5, true),
    ];
    assert!((ece(&s) - 100.0 * (4.0 / 6.0 * 0.15 + 2.0 / 6.0 * 0.5)).abs() < 1e-4);
    assert_eq!(ece(&[]), 0.0);
    assert!(ece(&[(1.0, true), (f32::NAN, false)]) >= 0.0);
}

#[test]
fn cues_read_the_aspect_layout() {
    let missing = DspCues::new(&AspectVectors::default(), None, None, 1.0);
    assert!(missing.missing && !missing.silent);
    let mut v = AspectVectors {
        spectrum: vec![0.0; SPECTRUM_DIM],
        timbre: vec![0.0; TIMBRE_DIM],
        pitch: vec![0.0; PITCH_DIM],
        amplitude: vec![0.0; AMPLITUDE_DIM],
    };
    v.amplitude[amplitude::LOUDNESS] = -100.0;
    let silent = DspCues::new(&v, Some(Kind::Loop), Some(0.8), 2.0);
    assert!(silent.silent && !silent.missing);
    assert!((silent.p_loop - 0.8).abs() < 1e-6 && (silent.log2_dur - 1.0).abs() < 1e-6);
    v.amplitude[amplitude::LOUDNESS] = -20.0;
    v.amplitude[amplitude::DECAY] = -0.5;
    let c = DspCues::new(&v, Some(Kind::OneShot), Some(0.9), f64::NAN);
    assert!(!c.silent && (c.p_loop - 0.1).abs() < 1e-6 && c.log2_dur == 0.0);
    assert_eq!(c.decay, -0.5);
}
