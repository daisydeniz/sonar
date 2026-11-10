//! Pause, cancel and panic containment.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
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
fn pause_mid_batch_commits_finished_files_and_resume_redoes_none() {
    let env = Env::new(40, 0);
    // The hook parks the worker on the 3rd file of the first batch; the test
    // pauses while it is in flight, then lets it finish.
    let (reached_tx, reached_rx) = mpsc::channel::<()>();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let reached_tx = Mutex::new(reached_tx);
    let release_rx = Mutex::new(release_rx);
    let calls = AtomicUsize::new(0);
    let job = start(
        &env.db,
        JobOptions {
            stages: vec![Stage::Meta],
            debug_before_file: Some(Arc::new(move |_, _| {
                if calls.fetch_add(1, Ordering::SeqCst) == 2 {
                    reached_tx.lock().expect("lock").send(()).expect("send");
                    release_rx.lock().expect("lock").recv().expect("release");
                }
            })),
            ..opts(1, 8)
        },
    )
    .expect("start");
    reached_rx.recv().expect("worker reached file 3");
    job.pause();
    release_tx.send(()).expect("release");

    // The 3 finished files are committed although the batch is incomplete;
    // the other 5 (and the whole look-ahead batch) were never started.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while job.progress().done < 3 && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    // Only checks that nothing else gets committed meanwhile; the wait above
    // is what synchronizes on the commit.
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(job.progress().done, 3);
    assert_eq!(env.count("SELECT count(*) FROM file_stages"), 3);
    assert_eq!(env.count("SELECT count(*) FROM audio_meta"), 3);
    assert!(!job.is_finished());

    job.resume();
    let s = job.wait().expect("job");
    assert_eq!((s.stages[0].done, s.stages[0].ok), (40, 40));
    assert_eq!(env.count("SELECT count(*) FROM file_stages"), 40);
    assert_eq!(
        env.count("SELECT count(DISTINCT file_id) FROM file_stages"),
        40
    );
}

#[test]
fn pause_mid_batch_with_skipped_file_counts_every_file_once() {
    // 2 threads, batch 8. One worker parks on s001; the other works through
    // its chunk (s006 is missing and comes back skipped) and parks on s007.
    // The test pauses while both are parked, so files still queued behind
    // s001 can only start after the resume: they are reported unstarted and
    // sit below the committed skipped s006. Resume must run exactly those and
    // neither re-run nor recount anything else.
    let env = Env::new(16, 0);
    fs::remove_file(env.root.join("s006.wav")).expect("rm");
    let (reached_tx, reached_rx) = mpsc::channel::<()>();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let reached_tx = Mutex::new(reached_tx);
    let release_rx = Mutex::new(release_rx);
    let parked: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];
    let resumed = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&resumed);
    // (file name, resume had been called when it started)
    let ran = Arc::new(Mutex::new(Vec::<(String, bool)>::new()));
    let ran_sink = Arc::clone(&ran);
    let events = Arc::new(Mutex::new(Vec::<(u64, u64, u64)>::new()));
    let sink = Arc::clone(&events);
    let job = start(
        &env.db,
        JobOptions {
            stages: vec![Stage::Meta],
            debug_before_file: Some(Arc::new(move |_, path| {
                let name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string();
                let after_resume = flag.load(Ordering::SeqCst);
                ran_sink
                    .lock()
                    .expect("lock")
                    .push((name.clone(), after_resume));
                let slot = match name.as_str() {
                    "s001.wav" => 0,
                    "s007.wav" => 1,
                    _ => return,
                };
                if !parked[slot].swap(true, Ordering::SeqCst) {
                    reached_tx.lock().expect("lock").send(()).expect("send");
                    release_rx.lock().expect("lock").recv().expect("release");
                }
            })),
            on_event: Some(Arc::new(move |e| {
                if let JobEvent::Progress {
                    done,
                    total,
                    skipped,
                    ..
                } = e
                {
                    sink.lock().expect("lock").push((*done, *total, *skipped));
                }
            })),
            ..opts(2, 8)
        },
    )
    .expect("start");
    // Relies on rayon's adaptive split + oldest-first stealing (s001 and s007
    // land on different workers, s002/s003 stay queued behind s001); time out
    // instead of hanging if a rayon change breaks that.
    reached_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("first worker parked");
    reached_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("second worker parked");
    job.pause();
    release_tx.send(()).expect("release");
    release_tx.send(()).expect("release");
    // Wait for the partial batch to commit, then resume.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while job.progress().done == 0 && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    let partial = job.progress().done;
    assert!(partial > 0 && partial < 16, "partial batch committed");
    resumed.store(true, Ordering::SeqCst);
    job.resume();
    let s = job.wait().expect("job");
    let st = &s.stages[0];
    assert_eq!((st.total, st.done, st.skipped, st.ok), (16, 16, 1, 15));
    assert_eq!(env.count("SELECT count(*) FROM file_stages"), 15);
    assert_eq!(
        env.count("SELECT count(DISTINCT file_id) FROM file_stages"),
        15
    );
    let ran = ran.lock().expect("lock");
    let mut names: Vec<&str> = ran.iter().map(|(n, _)| n.as_str()).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), ran.len(), "a file ran twice: {ran:?}");
    assert_eq!(ran.len(), 16, "every file ran once: {ran:?}");
    // The retry path ran a file below the committed skip (the rewind bug).
    assert!(
        ran.iter()
            .any(|(n, after)| *after && n.as_str() < "s006.wav"),
        "no file below s006 started after the resume: {ran:?}"
    );
    let ev = events.lock().expect("lock");
    assert!(ev.iter().all(|&(d, t, sk)| d <= t && sk <= 1), "{ev:?}");
    assert!(ev.windows(2).all(|w| w[0].0 <= w[1].0), "{ev:?}");
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
