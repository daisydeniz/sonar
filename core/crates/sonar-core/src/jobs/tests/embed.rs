//! The `embed` stage and the embedding store (ADR-027). Model-gated: these
//! tests print a note and pass when the CLAP files (or the `embed` feature)
//! are not available.

use std::fs;

use super::*;
use crate::embed::store::{dir_for_db, EmbedStore};
use crate::embed::EMBED_DIM;

/// Options for a 2-session pool, or `None` when the stage cannot run here.
fn embed_opts() -> Option<EmbedOptions> {
    let base = EmbedOptions {
        sessions: 2,
        ..EmbedOptions::default()
    };
    match base.unavailable_reason() {
        None => Some(base),
        Some(why) => {
            eprintln!("skipped: {why}");
            None
        }
    }
}

fn run_embed(env: &Env, embed: &EmbedOptions) -> JobSummary {
    env.run(JobOptions {
        stages: vec![Stage::Embed],
        embed: embed.clone(),
        ..opts(4, 4)
    })
}

fn live_ids(env: &Env) -> Vec<i64> {
    let r = EmbedStore::open_read(dir_for_db(&env.db)).expect("embedding store");
    let mut ids: Vec<i64> = r.file_ids().collect();
    ids.sort_unstable();
    ids
}

#[test]
fn embeds_every_readable_file_and_records_the_rest_as_stage_errors() {
    let Some(embed) = embed_opts() else { return };
    let env = Env::new(6, 2);
    let s = run_embed(&env, &embed);
    assert!(s.skipped.is_empty(), "{:?}", s.skipped);
    let st = &s.stages[0];
    assert_eq!(
        (st.stage, st.total, st.ok, st.errors),
        (Stage::Embed, 8, 6, 2)
    );

    let r = EmbedStore::open_read(dir_for_db(&env.db)).expect("store");
    assert_eq!((r.rows(), r.live_rows()), (6, 6));
    for (id, e) in r.iter() {
        assert!(id > 0);
        let norm: f32 = e.iter().map(|v| v * v).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-4, "file {id}: norm {norm}");
        assert!(e.iter().all(|v| v.is_finite()));
    }
    // Identical files give identical embeddings.
    let rows: Vec<&[f32; EMBED_DIM]> = r.iter().map(|(_, e)| e).collect();
    assert!(rows.windows(2).all(|w| w[0] == w[1]));

    // Failures are stage rows only: the file keeps the pipeline's status.
    assert_eq!(
        env.count("SELECT count(*) FROM file_stages WHERE stage='embed' AND outcome='error' AND message IS NOT NULL"),
        2
    );
    assert_eq!(
        env.count("SELECT count(*) FROM files WHERE status = 'error'"),
        0
    );
    // Nothing is left to do, and nothing is appended twice.
    let again = run_embed(&env, &embed);
    assert_eq!((again.stages[0].total, again.stages[0].done), (0, 0));
    assert_eq!(live_ids(&env).len(), 6);
}

#[test]
fn rerun_after_a_crash_between_append_and_commit_leaves_no_duplicates() {
    let Some(embed) = embed_opts() else { return };
    let env = Env::new(6, 0);
    run_embed(&env, &embed);
    let before = live_ids(&env);
    assert_eq!(before.len(), 6);

    // The store has rows whose SQLite batch never committed.
    env.conn()
        .execute(
            "DELETE FROM file_stages WHERE stage = 'embed' AND file_id IN (SELECT id FROM files ORDER BY id LIMIT 3)",
            [],
        )
        .expect("delete");
    let s = run_embed(&env, &embed);
    assert_eq!((s.stages[0].total, s.stages[0].ok), (3, 3));
    assert_eq!(live_ids(&env), before, "same files, one live row each");
    assert_eq!(
        env.count("SELECT count(*) FROM file_stages WHERE stage='embed'"),
        6
    );
    let r = EmbedStore::open_read(dir_for_db(&env.db)).expect("store");
    assert_eq!((r.rows(), r.live_rows()), (9, 6), "3 superseded rows");
}

#[test]
fn a_lost_embedding_store_requeues_the_files() {
    let Some(embed) = embed_opts() else { return };
    let env = Env::new(4, 0);
    run_embed(&env, &embed);
    fs::remove_dir_all(dir_for_db(&env.db)).expect("rm store");
    let s = run_embed(&env, &embed);
    assert_eq!((s.stages[0].total, s.stages[0].ok), (4, 4));
    assert_eq!(live_ids(&env).len(), 4);
}

#[test]
fn embed_has_the_lowest_priority() {
    let Some(embed) = embed_opts() else { return };
    let env = Env::new(8, 0);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    let s = env.run(JobOptions {
        stages: vec![Stage::Embed, Stage::Dsp, Stage::Meta],
        embed,
        debug_before_file: Some(Arc::new(move |stage, _| {
            if let Ok(mut v) = log.lock() {
                v.push(stage);
            }
        })),
        ..opts(4, 3)
    });
    let order: Vec<Stage> = s.stages.iter().map(|s| s.stage).collect();
    assert_eq!(order, [Stage::Meta, Stage::Dsp, Stage::Embed]);
    let seen = seen.lock().expect("lock").clone();
    let first_embed = seen.iter().position(|s| *s == Stage::Embed).expect("ran");
    assert!(seen[..first_embed].iter().all(|s| *s != Stage::Embed));
    assert!(seen[first_embed..].iter().all(|s| *s == Stage::Embed));
    assert_eq!(live_ids(&env).len(), 8);
}
