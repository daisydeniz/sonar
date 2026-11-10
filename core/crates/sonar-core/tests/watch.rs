//! T1.6 acceptance: the watcher keeps the index in line with the disk while
//! it runs (add / delete / rename / offline volume). Every step waits by
//! polling the database with a generous timeout; no fixed sleeps are
//! asserted on.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rusqlite::Connection;
use sonar_core::fixtures::wav::{encode, SampleFormat, WavSpec};
use sonar_core::watch::{self, WatchEvent, WatchHandle, WatchOptions};
use sonar_core::Store;

const STEP_TIMEOUT: Duration = Duration::from_secs(10);

type Events = Arc<Mutex<Vec<(Instant, WatchEvent)>>>;

struct Env {
    _tmp: tempfile::TempDir,
    base: PathBuf,
    lib: PathBuf,
    library_id: i64,
    db: PathBuf,
    events: Events,
    handle: WatchHandle,
}

/// Distinct tiny WAV per `seed` (different content, so no move confusion).
fn wav(seed: u32) -> Vec<u8> {
    let step = 0.02 + seed as f32 * 0.003;
    let tone: Vec<f32> = (0..400).map(|i| 0.4 * (i as f32 * step).sin()).collect();
    encode(&WavSpec::new(16_000, 1, SampleFormat::Int16), &tone).bytes
}

fn write_wav(path: &Path, seed: u32) {
    fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    fs::write(path, wav(seed)).expect("write wav");
}

fn wait_until(what: &str, mut cond: impl FnMut() -> bool) {
    let start = Instant::now();
    while start.elapsed() < STEP_TIMEOUT {
        if cond() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("timed out after {STEP_TIMEOUT:?} waiting for: {what}");
}

impl Env {
    /// Library with `setup` run on its (canonical) root before the watcher
    /// starts; the watcher is started and its initial scan awaited.
    fn start(setup: impl FnOnce(&Path)) -> Self {
        let tmp = tempfile::tempdir().expect("tempdir");
        let base = fs::canonicalize(tmp.path()).expect("canonicalize");
        let lib = base.join("lib");
        fs::create_dir(&lib).expect("mkdir lib");
        setup(&lib);
        let db = base.join("home").join("sonar.db");
        let store = Store::open(&db).expect("open store");
        let library_id = store.add_library(&lib).expect("add library").id;
        drop(store);

        let events: Events = Arc::default();
        let sink = Arc::clone(&events);
        let options = WatchOptions {
            debounce: Duration::from_millis(100),
            max_delay: Duration::from_millis(300),
            root_poll: Duration::from_millis(100),
            error_retry: Duration::from_millis(500),
        };
        let handle = watch::start(&db, options, move |e| {
            sink.lock()
                .expect("events")
                .push((Instant::now(), e.clone()));
        })
        .expect("start watcher");
        let env = Self {
            _tmp: tmp,
            base,
            lib,
            library_id,
            db,
            events,
            handle,
        };
        wait_until("initial scan", || env.scans() >= 1);
        env
    }

    fn scans(&self) -> usize {
        self.events
            .lock()
            .expect("events")
            .iter()
            .filter(|(_, e)| matches!(e, WatchEvent::Scanned { .. }))
            .count()
    }

    fn has_event(&self, pred: impl Fn(&WatchEvent) -> bool) -> bool {
        self.events
            .lock()
            .expect("events")
            .iter()
            .any(|(_, e)| pred(e))
    }

    fn conn(&self) -> Connection {
        let c = Connection::open(&self.db).expect("open db");
        c.busy_timeout(Duration::from_secs(5)).expect("timeout");
        c
    }

    /// `(id, status)` of the row at `rel`, if any.
    fn row(&self, rel: &str) -> Option<(i64, String)> {
        self.conn()
            .query_row(
                "SELECT id, status FROM files WHERE library_id = ?1 AND rel_path = ?2",
                rusqlite::params![self.library_id, rel],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .ok()
    }

    fn online(&self) -> bool {
        self.conn()
            .query_row(
                "SELECT online FROM libraries WHERE id = ?1",
                [self.library_id],
                |r| r.get(0),
            )
            .expect("library row")
    }

    fn row_count(&self) -> i64 {
        self.conn()
            .query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))
            .expect("count")
    }
}

#[test]
fn add_and_delete_while_running() {
    let env = Env::start(|lib| write_wav(&lib.join("seed.wav"), 0));
    assert_eq!(env.row("seed.wav").expect("seeded").1, "new");

    // (a) add
    let t0 = Instant::now();
    write_wav(&env.lib.join("Drums").join("kick.wav"), 1);
    wait_until("added file indexed", || env.row("Drums/kick.wav").is_some());
    let status = env.row("Drums/kick.wav").expect("row").1;
    assert_eq!(status, "new");
    // Latency from the write to the Scanned event that reported the add.
    let latency_of = || {
        env.events
            .lock()
            .expect("events")
            .iter()
            .find(|(at, e)| {
                *at >= t0 && matches!(e, WatchEvent::Scanned { summary, .. } if summary.added == 1)
            })
            .map(|(at, _)| at.duration_since(t0))
    };
    wait_until("Scanned event reporting the add", || latency_of().is_some());
    let latency = latency_of();
    eprintln!("watch latency (write -> Scanned): {latency:?}");

    // (b) delete: row kept, marked missing
    let (id, _) = env.row("seed.wav").expect("row");
    fs::remove_file(env.lib.join("seed.wav")).expect("remove");
    wait_until("deleted file marked missing", || {
        env.row("seed.wav").is_some_and(|(_, s)| s == "missing")
    });
    assert_eq!(env.row("seed.wav").expect("kept").0, id, "row kept");
}

#[test]
fn rename_file_and_folder_keep_row_ids() {
    let env = Env::start(|lib| {
        write_wav(&lib.join("a.wav"), 10);
        write_wav(&lib.join("Sub").join("b.wav"), 11);
    });
    let a = env.row("a.wav").expect("a").0;
    let b = env.row("Sub/b.wav").expect("b").0;

    fs::rename(env.lib.join("a.wav"), env.lib.join("renamed.wav")).expect("rename file");
    wait_until("file rename followed", || env.row("renamed.wav").is_some());
    assert_eq!(env.row("renamed.wav").expect("row").0, a);
    assert!(env.row("a.wav").is_none(), "old path row moved, not left");

    fs::rename(env.lib.join("Sub"), env.lib.join("Sub2")).expect("rename dir");
    wait_until("folder rename followed", || env.row("Sub2/b.wav").is_some());
    assert_eq!(env.row("Sub2/b.wav").expect("row").0, b);
    assert!(env.row("Sub/b.wav").is_none());
    assert_eq!(env.row_count(), 2, "no duplicates, nothing missing");
}

#[test]
fn offline_volume_keeps_rows_and_comes_back() {
    let env = Env::start(|lib| {
        write_wav(&lib.join("x.wav"), 20);
        write_wav(&lib.join("y.wav"), 21);
    });
    assert!(env.online());
    let x = env.row("x.wav").expect("x").0;

    // Unmount-like: the root disappears.
    let away = env.base.join("lib_away");
    fs::rename(&env.lib, &away).expect("rename root away");
    wait_until("library goes offline", || !env.online());
    wait_until("Offline event", || {
        env.has_event(|e| matches!(e, WatchEvent::Offline { .. }))
    });
    assert_eq!(env.row("x.wav"), Some((x, "new".into())), "rows kept");
    assert_eq!(env.row("y.wav").expect("y").1, "new", "not marked missing");

    // Remount: root back, watch re-established, scan flips online.
    fs::rename(&away, &env.lib).expect("rename root back");
    wait_until("library back online", || env.online());
    wait_until("Online event", || {
        env.has_event(|e| matches!(e, WatchEvent::Online { .. }))
    });
    assert_eq!(env.row("x.wav"), Some((x, "new".into())));

    // The re-established watch sees new files.
    write_wav(&env.lib.join("after.wav"), 22);
    wait_until("add after remount indexed", || {
        env.row("after.wav").is_some()
    });
}

#[test]
fn library_added_after_start_is_watched_and_stop_is_idempotent() {
    let env = Env::start(|_| {});
    let second = env.base.join("lib2");
    fs::create_dir(&second).expect("mkdir");
    write_wav(&second.join("k.wav"), 30);
    let store = Store::open(&env.db).expect("open");
    let id = store.add_library(&second).expect("add").id;
    env.handle.refresh_libraries();
    wait_until("new library scanned", || {
        env.has_event(|e| matches!(e, WatchEvent::Scanned { library_id, .. } if *library_id == id))
    });
    write_wav(&second.join("k2.wav"), 31);
    wait_until("file in new library indexed", || {
        env.conn()
            .query_row(
                "SELECT COUNT(*) FROM files WHERE library_id = ?1",
                [id],
                |r| r.get::<_, i64>(0),
            )
            .is_ok_and(|n| n == 2)
    });
    assert!(env.handle.is_running());
    env.handle.stop();
    env.handle.stop();
    assert!(!env.handle.is_running());
}

#[test]
fn stop_from_inside_the_callback_does_not_deadlock() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let base = fs::canonicalize(tmp.path()).expect("canon");
    let lib = base.join("lib");
    fs::create_dir(&lib).expect("mkdir");
    let db = base.join("sonar.db");
    let store = Store::open(&db).expect("open");
    store.add_library(&lib).expect("add");
    drop(store);
    let slot: Arc<Mutex<Option<Arc<WatchHandle>>>> = Arc::default();
    let in_cb = Arc::clone(&slot);
    let handle = Arc::new(
        watch::start(&db, WatchOptions::default(), move |_| {
            let h = in_cb.lock().expect("slot").clone();
            if let Some(h) = h {
                h.stop(); // on the watcher thread: must not self-join
            }
        })
        .expect("start"),
    );
    *slot.lock().expect("slot") = Some(Arc::clone(&handle));
    handle.rescan_now();
    wait_until("watcher ended itself", || !handle.is_running());
    *slot.lock().expect("slot") = None;
    handle.stop();
}

#[test]
fn panicking_callback_does_not_kill_the_watcher() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let base = fs::canonicalize(tmp.path()).expect("canon");
    let lib = base.join("lib");
    fs::create_dir(&lib).expect("mkdir");
    let db = base.join("sonar.db");
    let store = Store::open(&db).expect("open");
    store.add_library(&lib).expect("add");
    drop(store);
    let calls = Arc::new(Mutex::new(0_u32));
    let c = Arc::clone(&calls);
    let options = WatchOptions {
        debounce: Duration::from_millis(50),
        root_poll: Duration::from_millis(100),
        ..WatchOptions::default()
    };
    let handle = watch::start(&db, options, move |_| {
        *c.lock().expect("calls") += 1;
        panic!("callback bug");
    })
    .expect("start");
    wait_until("first callback", || *calls.lock().expect("calls") >= 1);
    handle.rescan_now();
    wait_until("watcher still delivers", || {
        *calls.lock().expect("calls") >= 2
    });
    assert!(handle.is_running());
}
