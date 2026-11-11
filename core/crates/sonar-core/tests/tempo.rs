//! BPM accuracy on the generated fixture loops (`sonar fixtures`), with
//! filenames hidden (no BPM prior, no kind hint), gated by the loop/one-shot
//! classifier exactly as the pipeline does. AC (T3.4): ≥ 95 % of the loops
//! within ±1 BPM of `truth.bpm`; the octave-error rate (×½, ×2, ×⅓, ×3,
//! ×⅔, ×3⁄2 within ±1 BPM) is reported separately; no file classified
//! one-shot gets a tempo.
//!
//! Run with `--nocapture` to see the per-file table.

use sonar_core::classify::{classify_kind, Kind};
use sonar_core::decode::{DecodeOptions, Decoder, ANALYSIS_RATE};
use sonar_core::dsp::Analyzer;
use sonar_core::fixtures::manifest::Kind as TruthKind;
use sonar_core::fixtures::{generate, FixtureOptions};
use sonar_core::tempo::loop_tempo;

/// Required ±1 BPM accuracy on the fixture loops.
const MIN_ACCURACY: f64 = 0.95;
/// Tolerance (BPM).
const TOL: f64 = 1.0;
/// Metrical-level ratios counted as octave errors.
const OCTAVE_RATIOS: [f64; 6] = [0.5, 2.0, 1.0 / 3.0, 3.0, 2.0 / 3.0, 1.5];

#[test]
fn fixture_loops_bpm_without_filenames() {
    let tmp = tempfile::tempdir().expect("tmp");
    let manifest = generate(tmp.path(), &FixtureOptions::default()).expect("fixtures");
    let opts = DecodeOptions::default();
    let mut decoder = Decoder::new();
    let mut analyzer = Analyzer::new(ANALYSIS_RATE).expect("analyzer");
    let (mut loops, mut correct, mut octave) = (0usize, 0usize, 0usize);
    let mut wrong = Vec::new();
    let mut oneshot_with_bpm = Vec::new();
    for f in &manifest.fixtures {
        if f.truth.corrupt.is_some() {
            continue;
        }
        let Ok(d) = decoder.decode(&tmp.path().join(&f.path), &opts) else {
            continue;
        };
        let feats = analyzer.analyze(&d.samples).expect("features");
        let kind = classify_kind(&feats, d.source.duration_s, None);
        let tempo = loop_tempo(&kind, &feats, d.source.duration_s, None);
        let truth_one_shot = f.truth.kind == Some(TruthKind::OneShot);
        if (kind.kind == Kind::OneShot || truth_one_shot) && tempo.is_some() {
            oneshot_with_bpm.push(f.path.clone());
        }
        let (Some(TruthKind::Loop), Some(want)) = (f.truth.kind, f.truth.bpm) else {
            continue;
        };
        loops += 1;
        let got = tempo.map(|t| f64::from(t.bpm));
        let ok = got.is_some_and(|g| (g - want).abs() <= TOL);
        let oct =
            !ok && got.is_some_and(|g| OCTAVE_RATIOS.iter().any(|r| (g - want * r).abs() <= TOL));
        println!(
            "{:<26} want {:6.2} got {:>7} conf {:>5} beats {:>4} {:?} {}",
            f.path,
            want,
            got.map_or("-".into(), |g| format!("{g:.2}")),
            tempo.map_or("-".into(), |t| format!("{:.2}", t.confidence)),
            tempo
                .and_then(|t| t.beats)
                .map_or("-".into(), |b| b.to_string()),
            kind.kind,
            if ok {
                ""
            } else if oct {
                "OCTAVE"
            } else {
                "WRONG"
            }
        );
        correct += usize::from(ok);
        octave += usize::from(oct);
        if !ok {
            wrong.push(f.path.clone());
        }
    }
    let acc = correct as f64 / loops.max(1) as f64;
    println!(
        "BPM ±{TOL}: {correct}/{loops} = {:.1} %; octave errors {octave}/{loops} = {:.1} %; wrong: {wrong:?}",
        acc * 100.0,
        octave as f64 / loops.max(1) as f64 * 100.0
    );
    assert!(loops >= 30, "too few fixture loops: {loops}");
    assert!(
        oneshot_with_bpm.is_empty(),
        "one-shots with a BPM: {oneshot_with_bpm:?}"
    );
    assert!(
        acc >= MIN_ACCURACY,
        "accuracy {acc:.3} < {MIN_ACCURACY}: {wrong:?}"
    );
}
