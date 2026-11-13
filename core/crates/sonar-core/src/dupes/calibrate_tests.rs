//! Threshold calibration of the near-duplicate pass (ADR-036): prints the
//! per-aspect distances of true variant pairs and of the nearest pairs of
//! different sounds on the test library ([`super::test_lib`]).
//! `cargo test -p sonar-core --lib dupes::calibrate -- --ignored --nocapture`

use std::collections::HashMap;

use super::distance::{AspectDists, Metric};
use super::test_lib::lib;
use crate::index::vectors::{dir_for_db, VectorStore};
use crate::Store;

struct Pair {
    a: i64,
    b: i64,
    d: AspectDists,
    dur: (i64, i64),
}

fn max(d: &AspectDists) -> f32 {
    d.iter().fold(0.0f32, |m, &x| m.max(x))
}

fn mean(d: &AspectDists) -> f32 {
    d.iter().sum::<f32>() / 4.0
}

#[test]
#[ignore = "calibration report: cargo test -p sonar-core --lib dupes::calibrate -- --ignored --nocapture"]
fn calibrate_near_threshold() {
    let lib = lib();
    let store = Store::open(&lib.db).expect("store");
    let vs = VectorStore::open_read(dir_for_db(&lib.db)).expect("vectors");
    let metric = Metric::new(&vs).expect("metric");
    let durations: HashMap<i64, i64> = store
        .conn()
        .prepare("SELECT file_id, duration_ms FROM audio_meta WHERE duration_ms IS NOT NULL")
        .and_then(|mut s| s.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect())
        .expect("durations");
    let mut ids: Vec<i64> = lib
        .labels
        .keys()
        .copied()
        .filter(|id| vs.row_of(*id).is_some() && durations.contains_key(id))
        .collect();
    ids.sort_unstable();
    let row = |id: i64| vs.row_of(id).and_then(|r| usize::try_from(r).ok());
    let mut pos = Vec::new();
    let mut neg = Vec::new();
    for (i, &a) in ids.iter().enumerate() {
        for &b in &ids[i + 1..] {
            let (Some(ra), Some(rb)) = (row(a), row(b)) else {
                continue;
            };
            let d = metric.per_aspect(ra, rb).expect("rows");
            let p = Pair {
                a,
                b,
                d,
                dur: (durations[&a], durations[&b]),
            };
            if lib.labels[&a] == lib.labels[&b] {
                pos.push(p);
            } else {
                neg.push(p);
            }
        }
    }
    let show = |p: &Pair| {
        println!(
            "  max {:.3} mean {:.3}  [sp {:.3} ti {:.3} pi {:.3} am {:.3}]  dur {}/{}  {} | {}",
            max(&p.d),
            mean(&p.d),
            p.d[0],
            p.d[1],
            p.d[2],
            p.d[3],
            p.dur.0,
            p.dur.1,
            lib.paths[&p.a],
            lib.paths[&p.b]
        );
    };
    pos.sort_by(|x, y| max(&y.d).total_cmp(&max(&x.d)));
    println!("{} same-sound pairs, worst first:", pos.len());
    for p in pos
        .iter()
        .filter(|p| lib.labels[&p.a] != "silence")
        .take(12)
    {
        show(p);
    }
    let dd = pos
        .iter()
        .filter(|p| lib.labels[&p.a] != "silence")
        .map(|p| (p.dur.0 - p.dur.1).abs())
        .max()
        .unwrap_or(0);
    println!("largest duration difference of a same-sound pair: {dd} ms");
    neg.sort_by(|x, y| max(&x.d).total_cmp(&max(&y.d)));
    println!(
        "{} different-sound pairs, nearest first (any duration):",
        neg.len()
    );
    for p in neg.iter().take(12) {
        show(p);
    }
    println!("nearest different-sound pairs within 80 ms / 1 %:");
    let near_dur = |p: &&Pair| {
        let (a, b) = p.dur;
        (a - b).abs() <= 80.max(a.max(b) / 100)
    };
    for p in neg.iter().filter(near_dur).take(12) {
        show(p);
    }
    neg.sort_by(|x, y| mean(&x.d).total_cmp(&mean(&y.d)));
    println!("nearest by mean:");
    for p in neg.iter().take(10) {
        show(p);
    }
}
