//! `sonar scan` → `sonar search` / `sonar index` end to end in a temp
//! SONAR_HOME. Files only need the right names: scanning does not decode.

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

/// File names of the hits of `query` (JSON output).
fn hit_names(home: &Path, args: &[&str]) -> Vec<String> {
    let mut all = vec!["search", "--json"];
    all.extend_from_slice(args);
    let v: serde_json::Value = serde_json::from_str(&ok(&sonar(home, &all))).expect("json");
    v["hits"]
        .as_array()
        .expect("hits")
        .iter()
        .map(|h| {
            let p = h["path"].as_str().expect("path");
            Path::new(p)
                .file_name()
                .and_then(|n| n.to_str())
                .expect("name")
                .to_owned()
        })
        .collect()
}

#[test]
fn filters_sort_and_facets_from_filename_priors() {
    let tmp = tempfile::tempdir().expect("tmp");
    let lib = tmp.path().join("lib");
    fs::create_dir_all(lib.join("Drums")).expect("mkdir");
    for name in [
        "Bass_Loop_120BPM_Cmin.wav",
        "Lead 128 BPM Am.wav",
        "Pad 90 BPM Em.wav",
        "Chord Loop 126 BPM C.wav",
        "Drums/Kick_Hard.wav",
    ] {
        fs::write(lib.join(name), b"x").expect("write");
    }
    let home = tmp.path().join("home");
    ok(&sonar(&home, &["scan", lib.to_str().expect("utf-8")]));

    let sorted = |mut v: Vec<String>| {
        v.sort();
        v
    };
    assert_eq!(
        sorted(hit_names(&home, &["--bpm", "120-128"])),
        [
            "Bass_Loop_120BPM_Cmin.wav",
            "Chord Loop 126 BPM C.wav",
            "Lead 128 BPM Am.wav"
        ]
    );
    // Am + Camelot neighbours (Dm, Em) + relative major (C).
    assert_eq!(
        sorted(hit_names(&home, &["--key", "am", "--compatible"])),
        [
            "Chord Loop 126 BPM C.wav",
            "Lead 128 BPM Am.wav",
            "Pad 90 BPM Em.wav"
        ]
    );
    // Unknown BPM (the kick) comes last in both directions.
    assert_eq!(
        hit_names(&home, &["--sort", "bpm:desc"]),
        [
            "Lead 128 BPM Am.wav",
            "Chord Loop 126 BPM C.wav",
            "Bass_Loop_120BPM_Cmin.wav",
            "Pad 90 BPM Em.wav",
            "Kick_Hard.wav"
        ]
    );
    // Text + filter; options before the words.
    assert_eq!(
        hit_names(&home, &["--bpm", "-125", "loop"]),
        ["Bass_Loop_120BPM_Cmin.wav"]
    );

    let out = ok(&sonar(
        &home,
        &["search", "--json", "--facets", "--bpm", "100-"],
    ));
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    assert_eq!(v["total"], 3);
    let keys: Vec<(&str, u64)> = v["facets"]["keys"]
        .as_array()
        .expect("keys")
        .iter()
        .map(|k| {
            (
                k["value"].as_str().expect("v"),
                k["count"].as_u64().expect("n"),
            )
        })
        .collect();
    assert_eq!(keys, [("Am", 1), ("C", 1), ("Cm", 1)]);
    let text = ok(&sonar(&home, &["search", "--facets", "--key", "Cm"]));
    assert!(text.contains("1 of 1 results"), "{text}");
    assert!(text.contains("key: Am 1, C 1, Cm 1, Em 1"), "{text}");

    let err = sonar(&home, &["search", "--key", "kick"]);
    assert!(!err.status.success());
}

#[test]
fn scan_then_search_builds_and_refreshes_the_index() {
    let tmp = tempfile::tempdir().expect("tmp");
    let lib = tmp.path().join("lib");
    fs::create_dir_all(lib.join("Drums")).expect("mkdir");
    for name in [
        "Drums/Kick_Hard.wav",
        "Drums/BD_Deep.wav",
        "Drums/Vinyl Kick.wav",
        "Bass_Loop_120BPM_Cmin.wav",
    ] {
        fs::write(lib.join(name), b"x").expect("write");
    }
    let home = tmp.path().join("home");
    let lib_s = lib.to_str().expect("utf-8");
    ok(&sonar(&home, &["scan", lib_s]));

    // First search builds the index and says so on stderr.
    let out = sonar(&home, &["search", "bd"]);
    let text = ok(&out);
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("building the search index"),
        "{out:?}"
    );
    assert!(text.contains("Kick_Hard.wav"), "{text}");
    assert!(text.contains("3 of 3 results"), "{text}");

    let mut kicks = hit_names(&home, &["kick", "-vinyl"]);
    kicks.sort();
    assert_eq!(kicks, ["BD_Deep.wav", "Kick_Hard.wav"]);
    assert_eq!(
        hit_names(&home, &["cmin 120 bpm"]),
        ["Bass_Loop_120BPM_Cmin.wav"]
    );
    assert_eq!(
        hit_names(&home, &["--no-prefix", "kic"]),
        Vec::<String>::new()
    );
    // As-you-type: an unfinished word matches as a literal prefix only
    // (no synonyms, ADR-017); once complete it brings its synonyms (BD).
    let mut partial = hit_names(&home, &["kic"]);
    partial.sort();
    assert_eq!(partial, ["Kick_Hard.wav", "Vinyl Kick.wav"]);
    assert_eq!(hit_names(&home, &["kick"]).len(), 3);

    // Rescanning refreshes the existing index (no rebuild message).
    fs::rename(
        lib.join("Drums/Kick_Hard.wav"),
        lib.join("Drums/Snare_Hard.wav"),
    )
    .expect("rename");
    let text = ok(&sonar(&home, &["scan", lib_s]));
    assert!(text.contains("search index: 4 files"), "{text}");
    let out = sonar(&home, &["search", "snare"]);
    assert!(out.stderr.is_empty(), "{out:?}");
    assert!(ok(&out).contains("Snare_Hard.wav"));
    // Asking for CLAP without embeddings falls back to text with a note.
    let out = sonar(&home, &["search", "--mode", "semantic", "snare"]);
    let err = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(err.contains("text search only"), "{err}");
    let text = ok(&out);
    assert!(
        text.contains("Snare_Hard.wav") && text.contains(" ms, text"),
        "{text}"
    );

    let text = ok(&sonar(&home, &["index"]));
    assert!(text.starts_with("indexed 4 files"), "{text}");
    let synonyms = tmp.path().join("syn.tsv");
    fs::write(&synonyms, "# none\n").expect("write");
    let s = synonyms.to_str().expect("utf-8");
    assert!(hit_names(&home, &["--synonyms", s, "bd"]) == ["BD_Deep.wav"]);

    // <SONAR_HOME>/synonyms.tsv overrides the built-in groups.
    fs::write(home.join("synonyms.tsv"), "deep\tbass\n").expect("write");
    assert!(
        hit_names(&home, &["bd"]) == ["BD_Deep.wav"],
        "kick group gone"
    );
    let mut deep = hit_names(&home, &["deep"]);
    deep.sort();
    assert_eq!(deep, ["BD_Deep.wav", "Bass_Loop_120BPM_Cmin.wav"]);
}
