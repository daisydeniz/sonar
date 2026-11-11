use super::*;

fn set(v: &[usize]) -> HashSet<usize> {
    v.iter().copied().collect()
}

/// Hand-computed: ranking 5,1,7,2,9 with relevant {1,2,3}.
#[test]
fn precision_and_reciprocal_rank() {
    let ranked = [5, 1, 7, 2, 9];
    let rel = set(&[1, 2, 3]);
    assert_eq!(precision_at_k(&ranked, &rel, 1), 0.0);
    assert_eq!(precision_at_k(&ranked, &rel, 2), 0.5);
    assert_eq!(precision_at_k(&ranked, &rel, 4), 0.5);
    assert!((precision_at_k(&ranked, &rel, 3) - 1.0 / 3.0).abs() < 1e-12);
    assert_eq!(precision_at_k(&ranked, &rel, 0), 0.0);
    // k beyond the ranking still divides by k.
    assert_eq!(precision_at_k(&ranked, &rel, 10), 0.2);
    assert_eq!(reciprocal_rank(&ranked, &rel), 0.5);
    assert_eq!(reciprocal_rank(&ranked, &set(&[5])), 1.0);
    assert_eq!(reciprocal_rank(&ranked, &set(&[9])), 0.2);
    assert_eq!(reciprocal_rank(&ranked, &set(&[42])), 0.0);
}

/// 1-D toy pool: positions on a line, only `spectrum` carries signal
/// (other aspects constant), so distance = |x_i - x_j| (up to the
/// common z scale). Pool x = [0, 1, 2, 10, 11, 12]; families a,a,a,b,b,b.
fn line(xs: &[f32], fams: &[&str]) -> Vec<Entry> {
    xs.iter()
        .zip(fams)
        .enumerate()
        .map(|(i, (&x, f))| {
            let mut v = AspectVectors {
                spectrum: vec![0.0; crate::dsp::aspects::SPECTRUM_DIM],
                timbre: vec![0.0; crate::dsp::aspects::TIMBRE_DIM],
                pitch: vec![0.0; crate::dsp::aspects::PITCH_DIM],
                amplitude: vec![0.0; crate::dsp::aspects::AMPLITUDE_DIM],
            };
            v.spectrum[0] = x;
            Entry {
                path: PathBuf::from(format!("f{i}.wav")),
                vectors: v,
                family: Some((*f).to_string()),
                note_midi: None,
            }
        })
        .collect()
}

#[test]
fn well_separated_families_score_100() {
    let e = line(
        &[0.0, 1.0, 2.0, 10.0, 11.0, 12.0],
        &["a", "a", "a", "b", "b", "b"],
    );
    let m = compute(&e, &[], false).unwrap();
    assert_eq!((m.queries, m.pool), (6, 6));
    assert!((m.p10_pct - 100.0).abs() < 1e-9 && (m.mrr_pct - 100.0).abs() < 1e-9);
    assert!(m.pitch.is_none() && m.sweep.is_empty());
}

/// The entry at x = 2.5 is labeled "b" though it sits next to the "a"
/// files; hand-computed values below.
#[test]
fn mislabeled_entry_lowers_both_metrics() {
    // x:     0    1    2.5  10   11
    // fam:   a    a    b    b    b
    let e = line(&[0.0, 1.0, 2.5, 10.0, 11.0], &["a", "a", "b", "b", "b"]);
    let m = compute(&e, &[], false).unwrap();
    // Seed 0 (a): R={1}, k=1, ranking 1,2,.. -> P=1, RR=1.
    // Seed 1 (a): R={0}, k=1, ranking 0,2,.. -> P=1, RR=1.
    // Seed 2 (b): R={3,4}, k=2, ranking 1,0,3,4 -> P@2=0, RR=1/3.
    // Seed 3 (b): R={2,4}, k=2, ranking 4,2,.. -> P@2=1, RR=1.
    // Seed 4 (b): R={2,3}, k=2, ranking 3,2,.. -> P@2=1, RR=1.
    assert_eq!(m.queries, 5);
    assert!((m.p10_pct - 100.0 * 4.0 / 5.0).abs() < 1e-9, "{m:?}");
    assert!(
        (m.mrr_pct - 100.0 * (1.0 + 1.0 + 1.0 / 3.0 + 1.0 + 1.0) / 5.0).abs() < 1e-9,
        "{m:?}"
    );
}

#[test]
fn singleton_families_and_missing_seeds_do_not_count() {
    let e = line(&[0.0, 1.0, 50.0], &["a", "a", "solo"]);
    let m = compute(&e, &[], false).unwrap();
    assert_eq!(m.queries, 2);
    assert!(compute(&line(&[0.0], &["a"]), &[], false).is_none());
}

#[test]
fn golden_queries_use_listed_files_and_count_skips() {
    let e = line(&[0.0, 1.0, 2.0, 10.0], &["x", "x", "x", "x"]);
    let given = vec![
        SimilarQuery {
            query: "f0.wav".into(),
            relevant: vec!["f1.wav".into(), "gone.wav".into()],
        },
        SimilarQuery {
            query: "nope.wav".into(),
            relevant: vec!["f1.wav".into()],
        },
        // The far file is the only relevant one: rank 3 of 3.
        SimilarQuery {
            query: "f0.wav".into(),
            relevant: vec!["f3.wav".into()],
        },
    ];
    let m = compute(&e, &given, true).unwrap();
    assert_eq!(m.queries, 2);
    assert_eq!(m.skipped, 2, "one missing relevant file, one missing seed");
    assert!((m.p10_pct - 50.0).abs() < 1e-9, "{m:?}");
    assert!((m.mrr_pct - 100.0 * (1.0 + 1.0 / 3.0) / 2.0).abs() < 1e-9);
    assert_eq!(m.sweep.len(), sweep_grid().len());
}

#[test]
fn jsonl_parsing() {
    let text = concat!(
        "{\"query\": \"a/k1.wav\", \"relevant\": [\"a/k2.wav\", \"/abs/k3.wav\"]}\n",
        "\n",
        "not json\n",
        "{\"query\": \"b.wav\", \"relevant\": []}\n",
    );
    let (q, w) = parse_similar_jsonl(text, std::path::Path::new("/gold"));
    assert_eq!(q.len(), 1);
    assert_eq!(q[0].query, PathBuf::from("/gold/a/k1.wav"));
    assert_eq!(
        q[0].relevant,
        [
            PathBuf::from("/gold/a/k2.wav"),
            PathBuf::from("/abs/k3.wav")
        ]
    );
    assert_eq!(w.len(), 2, "{w:?}");
    assert!(w[0].contains("line 3") && w[1].contains("line 4"));
}

#[test]
fn example_file_parses() {
    let text = include_str!("../../../../../eval/golden/similar.example.jsonl");
    let (q, w) = parse_similar_jsonl(text, std::path::Path::new("."));
    assert!(w.is_empty(), "{w:?}");
    assert_eq!(q.len(), 1);
    assert_eq!(q[0].relevant.len(), 2);
}

/// `golden_dataset` picks up `similar.jsonl`: queries, files only named
/// there become extras, malformed lines become warnings.
#[test]
fn golden_dir_loads_similar_jsonl() {
    use crate::eval::golden::golden_dataset;
    let tmp = tempfile::tempdir().unwrap();
    let d = tmp.path();
    for f in ["a.wav", "b.wav", "c.wav"] {
        std::fs::write(d.join(f), b"x").unwrap();
    }
    std::fs::write(
        d.join("labels.csv"),
        "path,kind\na.wav,loop\nb.wav,oneshot\n",
    )
    .unwrap();
    std::fs::write(
        d.join("similar.jsonl"),
        "{\"query\":\"a.wav\",\"relevant\":[\"b.wav\",\"c.wav\",\"gone.wav\"]}\noops\n",
    )
    .unwrap();
    let ds = golden_dataset(d).unwrap();
    assert_eq!(ds.items.len(), 2);
    assert_eq!(ds.similar_queries.len(), 1);
    let extra: Vec<_> = ds.similar_extra.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(extra, ["c.wav"], "only c.wav is new and exists");
    assert_eq!(ds.warnings.len(), 1, "{:?}", ds.warnings);
    assert!(ds.has_similarity());
    // Without the file there is nothing to measure.
    std::fs::remove_file(d.join("similar.jsonl")).unwrap();
    assert!(!golden_dataset(d).unwrap().has_similarity());
}
