//! `sonar watch`: argument parsing and a short live run.

use std::fs;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use sonar_core::fixtures::wav::{encode, SampleFormat, WavSpec};

fn sonar() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_sonar"));
    c.env_remove("RUST_LOG");
    c
}

/// Kills and reaps the child on drop, so a failed assertion cannot leak it.
struct KillOnDrop(Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn help_lists_options() {
    let out = sonar().args(["watch", "--help"]).output().expect("spawn");
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("--json") && text.contains("--debounce-ms"),
        "{text}"
    );
}

#[test]
fn rejects_zero_debounce() {
    let out = sonar()
        .args(["watch", "--debounce-ms", "0"])
        .output()
        .expect("spawn");
    assert!(!out.status.success());
}

#[test]
fn reports_added_file_as_json_lines() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let base = fs::canonicalize(tmp.path()).expect("canon");
    let lib = base.join("lib");
    fs::create_dir(&lib).expect("mkdir");
    let home = base.join("home");
    let tone: Vec<f32> = (0..400).map(|i| 0.4 * (i as f32 * 0.05).sin()).collect();
    let bytes = encode(&WavSpec::new(16_000, 1, SampleFormat::Int16), &tone).bytes;
    fs::write(lib.join("one.wav"), &bytes).expect("write");

    let scan = sonar()
        .args(["scan", lib.to_str().expect("utf-8")])
        .env("SONAR_HOME", &home)
        .output()
        .expect("scan");
    assert!(scan.status.success());

    let mut child = KillOnDrop(
        sonar()
            .args(["watch", "--json", "--debounce-ms", "100"])
            .env("SONAR_HOME", &home)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn watch"),
    );
    let stdout = child.0.stdout.take().expect("stdout");
    let (tx, rx) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });

    let next = |rx: &mpsc::Receiver<String>, what: &str| -> serde_json::Value {
        let line = rx
            .recv_timeout(Duration::from_secs(15))
            .unwrap_or_else(|_| panic!("timed out waiting for {what}"));
        serde_json::from_str(&line).unwrap_or_else(|e| panic!("bad JSON {line:?}: {e}"))
    };

    // Initial scan: nothing new.
    let first = next(&rx, "initial scan");
    assert_eq!(first["event"], "scanned");
    assert_eq!(first["summary"]["added"], 0);

    fs::write(
        lib.join("two.wav"),
        encode(
            &WavSpec::new(16_000, 1, SampleFormat::Int16),
            &tone.iter().map(|x| x * 0.5).collect::<Vec<f32>>(),
        )
        .bytes,
    )
    .expect("write");
    let start = Instant::now();
    let added = loop {
        let ev = next(&rx, "scan reporting the new file");
        if ev["event"] == "scanned" && ev["summary"]["added"] == 1 {
            break ev;
        }
        assert!(start.elapsed() < Duration::from_secs(15));
    };
    assert_eq!(added["library_id"], 1);

    // `child` is killed and reaped when it drops, also if an assertion fails.
}
