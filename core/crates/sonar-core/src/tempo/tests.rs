//! Unit tests: analytic click trains (whole loops, free-running and
//! truncated windows), priors, gating and degenerate input.

use super::*;
use crate::classify::classify_kind;
use crate::decode::ANALYSIS_RATE;
use crate::dsp::Analyzer;
use crate::text::PriorSource;

const SR: f32 = ANALYSIS_RATE as f32;

/// `secs` of 10 ms decaying 1 kHz clicks every `60/bpm` s (first at 0),
/// accent (×2) every `accent_every` beats.
fn clicks(bpm: f64, secs: f64, accent_every: usize) -> Vec<f32> {
    let n = (secs * f64::from(SR)).round() as usize;
    let mut x = vec![0.0f32; n];
    let len = (0.01 * SR) as usize;
    let mut beat = 0usize;
    loop {
        let start = (beat as f64 * 60.0 / bpm * f64::from(SR)).round() as usize;
        if start >= n {
            break;
        }
        let gain = if beat.is_multiple_of(accent_every) {
            0.8
        } else {
            0.4
        };
        for (i, s) in x[start..].iter_mut().take(len).enumerate() {
            let t = i as f32 / SR;
            *s += gain * (-t / 0.002).exp() * (2.0 * std::f32::consts::PI * 1000.0 * t).sin();
        }
        beat += 1;
    }
    x
}

fn tempo_of(samples: &[f32], duration_s: f64, prior: Option<BpmPrior>) -> Option<Tempo> {
    let mut analyzer = Analyzer::new(ANALYSIS_RATE).unwrap();
    let max = (30.0 * SR) as usize;
    let window = &samples[..samples.len().min(max)];
    let features = analyzer.analyze(window).unwrap();
    estimate_tempo(&features, duration_s, prior)
}

fn prior(bpm: f32) -> Option<BpmPrior> {
    Some(BpmPrior {
        bpm,
        explicit: true,
        source: PriorSource::Filename,
    })
}

#[test]
fn whole_click_loops_snap_to_exact_tempo() {
    for (bpm, beats) in [
        (70.0, 8u32),
        (87.5, 8),
        (120.0, 16),
        (133.0, 16),
        (150.0, 16),
        (175.0, 16),
    ] {
        let secs = f64::from(beats) * 60.0 / bpm;
        let t = tempo_of(&clicks(bpm, secs, 4), secs, None).expect("tempo");
        assert!((f64::from(t.bpm) - bpm).abs() < 0.01, "{bpm}: {t:?}");
        assert_eq!(t.beats, Some(beats), "{bpm}");
        assert_eq!(t.source, TempoSource::Detected);
        assert!(t.confidence > 0.3 && t.confidence <= 1.0, "{bpm}: {t:?}");
    }
}

#[test]
fn free_running_click_trains_within_one_bpm() {
    // Half a beat off a whole count: no loop snap, the comb peak decides.
    for bpm in [72.0, 87.5, 101.0, 133.0, 160.0, 175.0] {
        let secs = 14.5 * 60.0 / bpm;
        let t = tempo_of(&clicks(bpm, secs, 4), secs, None).expect("tempo");
        assert!((f64::from(t.bpm) - bpm).abs() <= 1.0, "{bpm}: {t:?}");
        assert_eq!(t.beats, None, "{bpm}");
    }
}

#[test]
fn truncated_window_uses_linear_autocorrelation() {
    // 40 s file, only the first 30 s analysed.
    for bpm in [87.5, 128.0, 140.0] {
        let t = tempo_of(&clicks(bpm, 40.0, 4), 40.0, None).expect("tempo");
        assert!((f64::from(t.bpm) - bpm).abs() <= 1.0, "{bpm}: {t:?}");
        assert_eq!(t.beats, None);
    }
}

#[test]
fn filename_prior_breaks_octave_ties_and_fills_in() {
    let secs = 16.0 * 60.0 / 168.0;
    let x = clicks(168.0, secs, 4);
    // Audio alone: 168.
    let t = tempo_of(&x, secs, None).unwrap();
    assert!((t.bpm - 168.0).abs() < 0.01, "{t:?}");
    // A name saying 84 picks the half-time octave, still detected.
    let t = tempo_of(&x, secs, prior(84.0)).unwrap();
    assert!((t.bpm - 84.0).abs() < 0.01, "{t:?}");
    assert_eq!(t.source, TempoSource::Detected);
    // An agreeing name raises the confidence.
    let alone = tempo_of(&x, secs, None).unwrap().confidence;
    let agreed = tempo_of(&x, secs, prior(168.0)).unwrap();
    assert!(agreed.confidence > alone, "{alone} vs {agreed:?}");
    // No rhythm at all: the name supplies the value.
    let silence = vec![0.0f32; (2.0 * SR) as usize];
    let t = tempo_of(&silence, 2.0, prior(124.0)).unwrap();
    assert_eq!((t.bpm, t.source), (124.0, TempoSource::Filename));
    assert!((t.confidence - NAME_CONF_EXPLICIT).abs() < 1e-6);
}

#[test]
fn one_shots_never_get_a_tempo() {
    let mut analyzer = Analyzer::new(ANALYSIS_RATE).unwrap();
    // A single decaying click: one-shot.
    let x = clicks(60.0, 0.5, 4);
    let f = analyzer.analyze(&x).unwrap();
    let kind = classify_kind(&f, 0.5, None);
    assert_eq!(kind.kind, Kind::OneShot);
    assert_eq!(loop_tempo(&kind, &f, 0.5, prior(120.0)), None);
    // A click loop: loop, with a tempo.
    let secs = 8.0 * 60.0 / 100.0;
    let x = clicks(100.0, secs, 4);
    let f = analyzer.analyze(&x).unwrap();
    let kind = classify_kind(&f, secs, None);
    assert_eq!(kind.kind, Kind::Loop);
    let t = loop_tempo(&kind, &f, secs, None).unwrap();
    assert!((t.bpm - 100.0).abs() < 0.01, "{t:?}");
}

#[test]
fn degenerate_input_gives_none_or_finite_values() {
    assert_eq!(tempo_of(&[], 0.0, None), None);
    assert_eq!(tempo_of(&[0.0; 100], 0.005, None), None);
    assert_eq!(tempo_of(&vec![0.0; (3.0 * SR) as usize], 3.0, None), None);
    let short = clicks(120.0, 0.3, 4);
    if let Some(t) = tempo_of(&short, 0.3, None) {
        assert!(t.bpm.is_finite() && t.confidence.is_finite());
    }
    let mut nan = clicks(120.0, 4.0, 4);
    for s in nan.iter_mut().step_by(97) {
        *s = f32::NAN;
    }
    for d in [4.0, f64::NAN, f64::INFINITY, -1.0] {
        if let Some(t) = tempo_of(&nan, d, None) {
            assert!(t.bpm.is_finite() && t.bpm > 0.0, "{d}: {t:?}");
            assert!((0.0..=1.0).contains(&t.confidence), "{d}: {t:?}");
        }
    }
    let p = Some(BpmPrior {
        bpm: f32::NAN,
        explicit: false,
        source: PriorSource::Filename,
    });
    assert_eq!(tempo_of(&[], 0.0, p), None);
}

#[test]
fn loop_fit_and_bar_weights() {
    assert_eq!(loop_fit(120.0, 8.0), Some((16, 120.0)));
    let (n, b) = loop_fit(121.0, 8.0).unwrap();
    assert_eq!((n, b), (16, 120.0));
    assert_eq!(loop_fit(124.0, 8.0), None); // 16.53 beats
    assert_eq!(loop_fit(120.0, 0.0), None);
    assert_eq!(loop_fit(f32::NAN, 8.0), None);
    assert_eq!(bar_weight(16), 1.0);
    assert_eq!(bar_weight(4), 1.0);
    assert!(bar_weight(12) < 1.0 && bar_weight(12) > bar_weight(9));
    assert!(bar_weight(9) > bar_weight(7));
    assert!(tempo_prior(PRIOR_CENTER_BPM) == 1.0 && tempo_prior(55.0) < tempo_prior(110.0));
}
