//! Vector store load benchmark (T4.1 AC: 1M × (dims) load < 1 s).

use std::time::Instant;

use sonar_core::dsp::aspects::{Aspect, AspectVectors, OVERALL_DIM};
use sonar_core::index::vectors::{VectorStore, RAW_ASPECTS};

const ROWS: i64 = 1_000_000;
const BATCH: i64 = 10_000;
/// AC budget for opening the store (map + id map + stats).
const LOAD_BUDGET_S: f64 = 1.0;

fn vecs(seed: i64) -> AspectVectors {
    let make = |a: Aspect| -> Vec<f32> {
        (0..a.dim())
            .map(|i| ((seed as f32) * 0.618 + i as f32 * 1.7).sin())
            .collect()
    };
    AspectVectors {
        spectrum: make(Aspect::Spectrum),
        timbre: make(Aspect::Timbre),
        pitch: make(Aspect::Pitch),
        amplitude: make(Aspect::Amplitude),
    }
}

/// Writes 1M rows (the library-wide raw dims of every aspect), then times
/// a read-only open (memory maps, id → row map, persisted stats) and a
/// first full pass over every matrix (all pages touched, warm OS cache).
#[test]
#[ignore = "benchmark: cargo test --release -p sonar-core --test vectors -- --ignored --nocapture"]
fn one_million_rows_load_under_a_second() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let t = Instant::now();
    {
        let (mut vs, _) = VectorStore::open(tmp.path()).expect("writer");
        let base: Vec<AspectVectors> = (0..BATCH).map(vecs).collect();
        for start in (1..=ROWS).step_by(BATCH as usize) {
            let batch: Vec<(i64, &AspectVectors)> = base
                .iter()
                .enumerate()
                .map(|(i, v)| (start + i as i64, v))
                .collect();
            vs.append(&batch).expect("append");
        }
    }
    let write_s = t.elapsed().as_secs_f64();

    let t = Instant::now();
    let vs = VectorStore::open_read(tmp.path()).expect("reader");
    let load_s = t.elapsed().as_secs_f64();
    assert_eq!(vs.live_rows(), ROWS as u64);

    let t = Instant::now();
    let mut sum = 0.0f64;
    for a in RAW_ASPECTS {
        let m = vs.matrix(a).expect("matrix");
        assert_eq!(m.len(), ROWS as usize * a.dim());
        sum += m.iter().map(|&x| f64::from(x)).sum::<f64>();
    }
    let scan_s = t.elapsed().as_secs_f64();

    let mb = vs.stats().bytes as f64 / 1e6;
    println!(
        "vector store: {ROWS} rows × {OVERALL_DIM} f32 ({mb:.0} MB): write {write_s:.2} s, \
         open {:.0} ms, first full scan {:.0} ms (checksum {sum:.1})",
        load_s * 1e3,
        scan_s * 1e3
    );
    assert!(load_s < LOAD_BUDGET_S, "open took {load_s:.3} s");
}
