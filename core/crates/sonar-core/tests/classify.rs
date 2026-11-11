//! Loop/one-shot accuracy on the generated fixture set (`sonar fixtures`,
//! incl. the 310 s file — only its first 30 s are analysed). Truth mapping
//! (ADR-020): `loop` → Loop; `one_shot`, `tone`, `noise` → OneShot;
//! `silence`, corrupt files and files without a `kind` are excluded.
//! Filenames are hidden from the classifier (no hint). AC: ≥ 95 %.
//!
//! Run with `--nocapture` to see the per-file table.

use sonar_core::classify::{classify_kind, Kind};
use sonar_core::decode::{DecodeOptions, Decoder, ANALYSIS_RATE};
use sonar_core::dsp::Analyzer;
use sonar_core::fixtures::manifest::Kind as TruthKind;
use sonar_core::fixtures::{generate, FixtureOptions};

/// Required accuracy on the included fixtures.
const MIN_ACCURACY: f64 = 0.95;

fn expected(truth: TruthKind) -> Option<Kind> {
    match truth {
        TruthKind::Loop => Some(Kind::Loop),
        TruthKind::OneShot | TruthKind::Tone | TruthKind::Noise => Some(Kind::OneShot),
        TruthKind::Silence => None,
    }
}

#[test]
fn fixture_accuracy_without_filenames() {
    let tmp = tempfile::tempdir().expect("tmp");
    let manifest = generate(tmp.path(), &FixtureOptions::default()).expect("fixtures");
    let opts = DecodeOptions::default();
    let mut decoder = Decoder::new();
    let mut analyzer = Analyzer::new(ANALYSIS_RATE).expect("analyzer");
    let (mut n, mut correct, mut loops) = (0usize, 0usize, 0usize);
    let mut wrong = Vec::new();
    for f in &manifest.fixtures {
        if f.truth.corrupt.is_some() {
            continue;
        }
        let Some(want) = f.truth.kind.and_then(expected) else {
            continue;
        };
        let d = decoder
            .decode(&tmp.path().join(&f.path), &opts)
            .unwrap_or_else(|e| panic!("{}: {e}", f.path));
        let feats = analyzer.analyze(&d.samples).expect("features");
        let r = classify_kind(&feats, d.source.duration_s, None);
        let s = &r.scores;
        let ok = r.kind == want;
        println!(
            "{:<26} want {:<8} got {:<8} p_loop {:.3} P {:.2} pulse {:5.2} dur {:6.2} tail {:>6} {}",
            f.path,
            format!("{want:?}"),
            format!("{:?}", r.kind),
            r.loop_probability,
            s.periodicity,
            s.pulse_db,
            s.duration_s,
            s.tail_db.map_or("-".into(), |t| format!("{t:.1}")),
            if ok { "" } else { "WRONG" }
        );
        n += 1;
        loops += usize::from(want == Kind::Loop);
        if ok {
            correct += 1;
        } else {
            wrong.push(f.path.clone());
        }
    }
    let acc = correct as f64 / n.max(1) as f64;
    println!(
        "loop/one-shot accuracy: {correct}/{n} = {:.1} % ({loops} loops); misclassified: {wrong:?}",
        acc * 100.0
    );
    assert!(
        n >= 80 && loops >= 15,
        "too few fixtures: {n} ({loops} loops)"
    );
    assert!(
        acc >= MIN_ACCURACY,
        "accuracy {acc:.3} < {MIN_ACCURACY}: {wrong:?}"
    );
}
