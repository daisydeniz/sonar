//! The `dsp` stage and the vector store (ADR-023).

use std::fs;

use super::*;
use crate::dsp::aspects::Aspect;
use crate::fixtures::{generate, FixtureOptions};
use crate::index::vectors::{dir_for_db, VectorStore};

fn reader(env: &Env) -> VectorStore {
    VectorStore::open_read(dir_for_db(&env.db)).expect("vector store")
}

fn dsp_ok_ids(env: &Env) -> Vec<i64> {
    let conn = env.conn();
    let mut stmt = conn
        .prepare(
            "SELECT file_id FROM file_stages WHERE stage = 'dsp' \
             AND outcome IN ('ok', 'partial') ORDER BY file_id",
        )
        .expect("prepare");
    let ids = stmt
        .query_map([], |r| r.get(0))
        .expect("query")
        .collect::<rusqlite::Result<Vec<i64>>>()
        .expect("ids");
    ids
}

/// Every `sonar fixtures` file the `dsp` stage analyzes gets one live row
/// of finite raw vectors of the right dimensions, and the persisted stats
/// cover exactly those rows.
#[test]
fn dsp_stage_stores_vectors_for_every_fixture() {
    let env = Env::new(0, 0);
    let opts = FixtureOptions {
        include_long: false,
        ..FixtureOptions::default()
    };
    generate(&env.root, &opts).expect("fixtures");
    let mut store = Store::open(&env.db).expect("store");
    scan_library(&mut store, 1).expect("scan");
    drop(store);
    let s = env.run(opts_threads());
    let dsp = &s.stages[1];
    assert!(dsp.ok > 20, "{dsp:?}");
    let ids = dsp_ok_ids(&env);
    let vs = reader(&env);
    assert_eq!(vs.live_rows(), ids.len() as u64);
    assert_eq!(vs.rows(), vs.live_rows());
    assert_eq!(vs.norm().count(), vs.rows());
    for id in ids {
        let v = vs
            .vectors(id)
            .unwrap_or_else(|| panic!("file {id}: no vectors"));
        for (aspect, x) in v.parts() {
            assert_eq!(x.len(), aspect.dim(), "file {id} {aspect:?}");
            assert!(x.iter().all(|f| f.is_finite()), "file {id} {aspect:?}");
        }
        let z = vs.normalized(id).expect("normalized");
        assert!(z.get(Aspect::Overall).iter().all(|f| f.is_finite()));
    }
}

fn opts_threads() -> JobOptions {
    opts(2, 8)
}

/// Re-analysis (version bump) supersedes rows and compacts the dead ones;
/// a lost store re-queues the analyzed files; files that leave the store
/// lose their rows.
#[test]
fn reanalysis_lost_store_and_removed_files_stay_consistent() {
    let env = Env::new(4, 0);
    env.run(opts_threads());
    assert_eq!((reader(&env).rows(), reader(&env).live_rows()), (4, 4));

    // Previous dsp version: all four re-analyzed; the old rows are dead
    // (half the store) and compacted away at the end of the stage.
    env.conn()
        .execute(
            "UPDATE file_stages SET version = ?1 WHERE stage = 'dsp'",
            [DSP_VERSION - 1],
        )
        .expect("age dsp");
    assert_eq!(env.run(opts_threads()).stages[1].total, 4);
    let vs = reader(&env);
    assert_eq!(
        (vs.rows(), vs.live_rows(), vs.stats().generation),
        (4, 4, 2)
    );
    drop(vs);

    // Store lost: the current-version results are re-queued and re-run.
    fs::remove_dir_all(dir_for_db(&env.db)).expect("rm vectors");
    let s = env.run(opts_threads());
    assert_eq!((s.stages[1].total, s.stages[1].ok), (4, 4));
    assert_eq!(reader(&env).live_rows(), 4);

    // A file whose dsp result is gone (deleted from the store) loses its
    // row at the start of the next stage run.
    env.conn()
        .execute_batch("PRAGMA foreign_keys = ON; DELETE FROM files WHERE rel_path = 's002.wav';")
        .expect("delete file");
    env.run(opts_threads());
    let vs = reader(&env);
    assert_eq!(vs.live_rows(), 3);
    assert_eq!(dsp_ok_ids(&env).len(), 3);
    for id in dsp_ok_ids(&env) {
        assert!(vs.row_of(id).is_some(), "file {id}");
    }
}
