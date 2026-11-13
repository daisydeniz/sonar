//! `sonar fav|hide|tag|collection|override` end to end in a temp SONAR_HOME:
//! what they set is in the next `sonar search`. Files only need the right
//! names (scanning does not decode; BPM/key come from the name priors).

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn sonar(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sonar"))
        .args(args)
        .env("SONAR_HOME", home)
        .env_remove("RUST_LOG")
        .output()
        .expect("spawn sonar")
}

fn ok(out: &Output) -> String {
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout.clone()).expect("utf-8")
}

fn err(out: &Output) -> String {
    assert!(!out.status.success(), "expected a failure");
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// Sorted file names of the hits of `sonar search --json <args>`.
fn names(home: &Path, args: &[&str]) -> Vec<String> {
    let mut all = vec!["search", "--json", "--mode", "text"];
    all.extend_from_slice(args);
    let v: serde_json::Value = serde_json::from_str(&ok(&sonar(home, &all))).expect("json");
    let mut out: Vec<String> = v["hits"]
        .as_array()
        .expect("hits")
        .iter()
        .filter_map(|h| h["path"].as_str())
        .filter_map(|p| Path::new(p).file_name().and_then(|n| n.to_str()))
        .map(str::to_owned)
        .collect();
    out.sort();
    out
}

#[test]
fn user_data_commands_show_up_in_the_next_search() {
    let tmp = tempfile::tempdir().expect("tmp");
    let lib = tmp.path().join("lib");
    fs::create_dir_all(&lib).expect("mkdir");
    for name in ["Lead 128 BPM Am.wav", "Pad 90 BPM Em.wav"] {
        fs::write(lib.join(name), b"x").expect("write");
    }
    let home = tmp.path().join("home");
    ok(&sonar(&home, &["scan", lib.to_str().expect("utf-8")]));
    // Build the index once; the commands below keep it current themselves.
    assert_eq!(names(&home, &[]).len(), 2);
    let lead = lib.join("Lead 128 BPM Am.wav");
    let lead = lead.to_str().expect("utf-8");

    // Favourite, by path.
    assert!(names(&home, &["--favorites"]).is_empty());
    ok(&sonar(&home, &["fav", lead]));
    assert_eq!(names(&home, &["--favorites"]), ["Lead 128 BPM Am.wav"]);
    ok(&sonar(&home, &["fav", "--off", lead]));
    assert!(names(&home, &["--favorites"]).is_empty());

    // BPM override, by id (the lead is file 1 in a fresh store).
    assert_eq!(names(&home, &["--bpm", "85-95"]), ["Pad 90 BPM Em.wav"]);
    ok(&sonar(&home, &["override", "bpm", "--set", "90", "1"]));
    assert_eq!(names(&home, &["--bpm", "85-95"]).len(), 2);
    let bad = err(&sonar(&home, &["override", "bpm", "--set", "fast", "1"]));
    assert!(bad.contains("not a plain number"), "{bad}");
    ok(&sonar(&home, &["override", "bpm", "--clear", "1"]));
    assert_eq!(names(&home, &["--bpm", "85-95"]), ["Pad 90 BPM Em.wav"]);

    // Tag, then hide.
    ok(&sonar(&home, &["tag", "add", "Dark Vibe", "1", "2"]));
    assert_eq!(names(&home, &["--tag", "dark vibe"]).len(), 2);
    assert!(ok(&sonar(&home, &["tag", "list"])).contains("Dark Vibe"));
    ok(&sonar(&home, &["tag", "rm", "dark vibe", "2"]));
    assert_eq!(
        names(&home, &["--tag", "dark vibe"]),
        ["Lead 128 BPM Am.wav"]
    );
    ok(&sonar(&home, &["hide", "2"]));
    assert_eq!(names(&home, &[]), ["Lead 128 BPM Am.wav"]);
    assert_eq!(names(&home, &["--include-hidden"]).len(), 2);
    ok(&sonar(&home, &["hide", "--off", "2"]));
    assert_eq!(names(&home, &[]).len(), 2);

    // Collections.
    let created = ok(&sonar(&home, &["collection", "create", "Session"]));
    assert!(created.starts_with("collection 1:"), "{created}");
    ok(&sonar(&home, &["collection", "add", "1", "2", lead]));
    let listed = ok(&sonar(&home, &["collection", "list", "1"]));
    let order: Vec<_> = listed
        .lines()
        .filter_map(|l| l.split_whitespace().next())
        .collect();
    assert_eq!(order, ["#2", "#1"]);
    assert!(ok(&sonar(&home, &["collection", "list"])).contains("Session"));
    assert!(err(&sonar(&home, &["collection", "add", "9", "1"])).contains("collection 9 not found"));
    ok(&sonar(&home, &["collection", "delete", "1"]));

    // Unknown file: an error, nothing changed.
    assert!(err(&sonar(&home, &["fav", "999"])).contains("file 999 not found"));
    assert!(err(&sonar(&home, &["fav", "/nowhere/x.wav"])).contains("not a file of any library"));
}
