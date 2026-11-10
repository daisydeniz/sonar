//! Pause, cancel and panic containment.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use super::*;

#[test]
fn pause_blocks_progress_and_resume_continues() {
    let env = Env::new(40, 0);
    let job = start(
        &env.db,
        JobOptions {
            stages: vec![Stage::Meta],
            debug_file_delay: Duration::from_millis(10),
            ..opts(2, 4)
        },
    )
    .expect("start");
    job.pause();
    assert!(job.is_paused() && job.progress().paused);
    // Let in-flight files drain, then progress must stand still.
    std::thread::sleep(Duration::from_millis(300));
    let frozen = job.progress().done;
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(job.progress().done, frozen, "no progress while paused");
    assert!(frozen < 40, "paused before the end");
    assert!(!job.is_finished());

    job.resume();
    assert!(!job.is_paused());
    let s = job.wait().expect("job");
    assert_eq!(s.stages[0].done, 40);
    assert_eq!(env.count("SELECT count(*) FROM file_stages"), 40);
}

#[test]
fn cancel_stops_early_and_leaves_consistent_state() {
    let env = Env::new(60, 0);
    // Deterministic: the hook parks the worker on the 9th file, the test
    // cancels while that file is in flight, then lets it continue.
    let (reached_tx, reached_rx) = mpsc::channel::<()>();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let reached_tx = Mutex::new(reached_tx);
    let release_rx = Mutex::new(release_rx);
    let calls = AtomicUsize::new(0);
    let job = start(
        &env.db,
        JobOptions {
            debug_before_file: Some(Arc::new(move |_, _| {
                if calls.fetch_add(1, Ordering::SeqCst) == 8 {
                    reached_tx.lock().expect("lock").send(()).expect("send");
                    release_rx.lock().expect("lock").recv().expect("release");
                }
            })),
            ..opts(1, 4)
        },
    )
    .expect("start");
    reached_rx.recv().expect("worker reached file 9");
    job.cancel();
    release_tx.send(()).expect("release");
    let s = job.wait().expect("job");
    assert!(s.cancelled);
    assert_eq!(s.stages.len(), 1, "dsp never started");
    // Batches 1-2 are committed. File 9 was in flight: its decode saw the
    // cancel flag and was skipped (stays pending); 10-12 never started.
    let done = env.count("SELECT count(*) FROM file_stages");
    assert_eq!(done, 8);
    assert_eq!(done, env.count("SELECT count(*) FROM audio_meta"));
    assert_eq!(
        done,
        env.count("SELECT count(*) FROM files WHERE status='meta'")
    );

    // Resuming finishes the rest without redoing the committed files.
    let rest = env.run(opts(2, 16));
    assert_eq!(rest.stages[0].total, 52);
    assert_eq!(
        env.count("SELECT count(*) FROM file_stages WHERE stage='meta'"),
        60
    );
    assert_eq!(
        env.count("SELECT count(*) FROM file_stages WHERE stage='dsp'"),
        60
    );
}

#[test]
fn panic_in_a_worker_becomes_an_error_row() {
    let env = Env::new(4, 0);
    let s = env.run(JobOptions {
        stages: vec![Stage::Meta],
        debug_before_file: Some(Arc::new(|_, path| {
            assert!(!path.ends_with("s002.wav"), "injected failure");
        })),
        ..opts(2, 8)
    });
    assert_eq!((s.stages[0].ok, s.stages[0].errors), (3, 1));
    let msg: String = env
        .conn()
        .query_row(
            "SELECT message FROM file_stages WHERE outcome = 'error'",
            [],
            |r| r.get(0),
        )
        .expect("error row");
    assert!(msg.starts_with("internal error"), "{msg}");
    assert_eq!(
        env.count("SELECT count(*) FROM files WHERE rel_path='s002.wav' AND status='error'"),
        1
    );
}
