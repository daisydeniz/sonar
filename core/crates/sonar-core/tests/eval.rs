//! Silver and golden loaders end to end on fixture files copied into a temp
//! dir: labels come from names / `labels.csv`, and the analyzer does not see
//! the names (deliberately wrong names must not change the audio estimate).

use std::path::Path;

use sonar_core::eval::{self, EvalReport};
use sonar_core::fixtures::manifest::{Fixture, Kind as TruthKind};
use sonar_core::fixtures::{generate, FixtureOptions};

fn copy(src: &Path, f: &Fixture, dest: &Path) {
    std::fs::create_dir_all(dest.parent().expect("parent")).expect("mkdir");
    std::fs::copy(src.join(&f.path), dest).expect("copy");
}

#[test]
fn silver_and_golden_use_labels_but_hide_names_from_the_analyzer() {
    let fx = tempfile::tempdir().expect("tmp");
    let opts = FixtureOptions {
        include_long: false,
        ..FixtureOptions::default()
    };
    let m = generate(fx.path(), &opts).expect("fixtures");
    let loops: Vec<&Fixture> = m
        .fixtures
        .iter()
        .filter(|f| f.path.starts_with("loops/") && f.truth.kind == Some(TruthKind::Loop))
        .filter(|f| f.truth.bpm.is_some_and(|b| b < 140.0))
        .take(3)
        .collect();
    assert_eq!(loops.len(), 3);

    // Silver: truthful names for two loops; the third is named with a BPM
    // far from its audio. A name-blind analyzer is right on the first two
    // and wrong on the third.
    let silver = tempfile::tempdir().expect("tmp");
    for (i, f) in loops.iter().enumerate() {
        let bpm = f.truth.bpm.expect("bpm");
        let named = if i < 2 { bpm } else { bpm * 0.6 }.round();
        copy(
            fx.path(),
            f,
            &silver
                .path()
                .join(format!("Packs/Groove_Loop_{named}bpm_{i}.wav")),
        );
    }
    std::fs::write(silver.path().join("Packs/readme.txt"), b"x").expect("write");
    let ds = eval::silver_dataset(silver.path()).expect("silver");
    assert_eq!((ds.items.len(), ds.unlabeled), (3, 0));
    let r = eval::evaluate(&ds);
    assert_eq!(r.unexpected_errors, 0, "{:?}", r.notes);
    let bpm = r.metrics.bpm.expect("bpm metric");
    assert_eq!((bpm.total, bpm.correct), (3, 2), "names must not leak");
    let kind = r.metrics.kind.expect("kind metric");
    assert_eq!((kind.total, kind.correct), (3, 3));
    assert!(r.metrics.key.is_none() && r.metrics.note.is_none());

    // Golden: labels.csv with relative paths, a quoted note, one missing
    // file and one bad cell.
    let golden = tempfile::tempdir().expect("tmp");
    let mut csv = String::from("path,category,kind,bpm,key,note,notes\n");
    for (i, f) in loops.iter().enumerate() {
        let name = format!("loop_{i}.wav");
        copy(fx.path(), f, &golden.path().join(&name));
        let bpm = f.truth.bpm.expect("bpm");
        csv.push_str(&format!("{name},drums,loop,{bpm},,,\"hand, labeled\"\n"));
    }
    csv.push_str("missing.wav,,oneshot,,,,\nloop_0.wav,,wobble,,,,\n");
    std::fs::write(golden.path().join("labels.csv"), csv).expect("csv");
    let ds = eval::golden_dataset(golden.path()).expect("golden");
    assert_eq!(
        (ds.items.len(), ds.missing.len(), ds.warnings.len()),
        (3, 1, 1)
    );
    let r = eval::evaluate(&ds);
    let bpm = r.metrics.bpm.expect("bpm metric");
    assert_eq!((bpm.total, bpm.correct), (3, 3));

    // The report renders both tiers and round-trips.
    let report = EvalReport::new("test", vec![eval::evaluate(&ds)]);
    assert!(report.to_markdown(None).contains("## golden"));
    let back: EvalReport = serde_json::from_str(&report.to_json()).expect("json");
    assert_eq!(back.tiers.len(), 1);
}
