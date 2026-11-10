//! Stage versions and error retry.

use std::sync::Arc;

use super::*;

#[test]
fn older_stage_version_is_pending_again_and_meta_keeps_levels() {
    let env = Env::new(3, 0);
    env.run(opts(1, 8));
    assert_eq!(
        env.count("SELECT count(*) FROM audio_meta WHERE peak_db IS NOT NULL"),
        3
    );
    env.conn()
        .execute(
            "UPDATE file_stages SET version = 0 WHERE stage = 'meta'",
            [],
        )
        .expect("age meta rows");
    let s = env.run(opts(1, 8));
    assert_eq!(s.stages[0].stage, Stage::Meta);
    assert_eq!(s.stages[0].total, 3);
    assert_eq!(s.stages[1].total, 0, "dsp rows are current");
    assert_eq!(
        env.count("SELECT count(*) FROM file_stages WHERE version = 0"),
        0
    );
    // Re-running `meta` must not erase peak/loudness, nor downgrade status.
    assert_eq!(
        env.count("SELECT count(*) FROM audio_meta WHERE peak_db IS NOT NULL"),
        3
    );
    assert_eq!(
        env.count("SELECT count(*) FROM files WHERE status='analyzed'"),
        3
    );
    assert_eq!(
        env.count("SELECT count(*) FROM file_stages"),
        6,
        "no duplicates"
    );
}

#[test]
fn errored_file_is_retried_only_after_a_version_bump() {
    let env = Env::new(0, 1);
    let first = env.run(opts(1, 8));
    assert_eq!(first.stages[0].errors, 1);
    assert_eq!(
        env.run(opts(1, 8)).stages[0].total,
        0,
        "error is not retried"
    );
    env.conn()
        .execute("UPDATE file_stages SET version = 0", [])
        .expect("age");
    assert_eq!(
        env.run(opts(1, 8)).stages[0].errors,
        1,
        "retried after bump"
    );
    assert_eq!(env.count("SELECT count(*) FROM file_stages"), 1);
}

#[test]
fn error_is_retried_only_when_its_own_stage_is_outdated() {
    let env = Env::new(2, 0);
    // s001 fails in dsp (injected); meta succeeds for both.
    let s = env.run(JobOptions {
        debug_before_file: Some(Arc::new(|stage, path| {
            assert!(
                !(stage == Stage::Dsp && path.ends_with("s001.wav")),
                "injected failure"
            );
        })),
        ..opts(1, 8)
    });
    assert_eq!((s.stages[1].ok, s.stages[1].errors), (1, 1));
    let status = |e: &Env| -> String {
        e.conn()
            .query_row(
                "SELECT status FROM files WHERE rel_path='s001.wav'",
                [],
                |r| r.get(0),
            )
            .expect("status")
    };
    assert_eq!(status(&env), "error");

    // An outdated meta row (ok) does not retry the dsp failure, nor itself.
    env.conn()
        .execute(
            "UPDATE file_stages SET version = 0 WHERE stage = 'meta' AND file_id = 2",
            [],
        )
        .expect("age meta");
    let s = env.run(opts(1, 8));
    assert_eq!((s.stages[0].total, s.stages[1].total), (0, 0));
    assert_eq!(status(&env), "error");

    // Outdated dsp error row: retried, and the file recovers.
    env.conn()
        .execute(
            "UPDATE file_stages SET version = 0 WHERE stage = 'dsp' AND file_id = 2",
            [],
        )
        .expect("age dsp");
    let s = env.run(opts(1, 8));
    assert_eq!((s.stages[1].total, s.stages[1].ok), (1, 1));
    assert_eq!(status(&env), "analyzed");
}
