//! `sonar similar` end to end: fixture one-shots and tonal notes scanned as
//! two libraries, analyzed, then queried by id, by external file, with
//! weights and with a library filter.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
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

fn json(home: &Path, args: &[&str]) -> serde_json::Value {
    serde_json::from_str(&ok(&sonar(home, args))).expect("json")
}

/// `group/name` of a result path.
fn short(p: &serde_json::Value) -> String {
    let p = PathBuf::from(p.as_str().expect("path"));
    let parent = p.parent().and_then(Path::file_name).expect("dir");
    let name = p.file_name().expect("name");
    format!("{}/{}", parent.to_string_lossy(), name.to_string_lossy())
}

fn hits(v: &serde_json::Value) -> Vec<(String, f64)> {
    v["hits"]
        .as_array()
        .expect("hits")
        .iter()
        .map(|h| (short(&h["path"]), h["distance"].as_f64().expect("distance")))
        .collect()
}

#[test]
fn similar_by_id_file_weights_and_filters() {
    let tmp = tempfile::tempdir().expect("tmp");
    let fx = tmp.path().join("fx");
    let home = tmp.path().join("home");
    let fx_s = fx.to_str().expect("utf-8");
    ok(&sonar(&home, &["fixtures", fx_s, "--skip-long"]));
    let (shots, tonal) = (fx.join("oneshots"), fx.join("tonal"));
    for dir in [&shots, &tonal] {
        ok(&sonar(&home, &["scan", dir.to_str().expect("utf-8")]));
    }
    ok(&sonar(&home, &["analyze"]));

    let all = json(&home, &["search", "--json", "--limit", "1000"]);
    let ids: HashMap<String, i64> = all["hits"]
        .as_array()
        .expect("hits")
        .iter()
        .map(|h| (short(&h["path"]), h["file_id"].as_i64().expect("id")))
        .collect();
    let kick = ids["oneshots/hit_000.wav"].to_string();

    // Kicks are hit_000..hit_003 (fixture catalog order).
    let v = json(&home, &["similar", &kick, "--json", "--limit", "3"]);
    let near = hits(&v);
    assert_eq!(
        v["seed"]["file_id"].as_i64(),
        Some(ids["oneshots/hit_000.wav"])
    );
    assert_eq!(v["candidates"].as_u64(), Some(ids.len() as u64 - 1));
    assert!(
        [
            "oneshots/hit_001.wav",
            "oneshots/hit_002.wav",
            "oneshots/hit_003.wav"
        ]
        .contains(&near[0].0.as_str()),
        "{near:?}"
    );

    // External copy (here: the file itself) → itself first at ≈ 0.
    let file = shots.join("hit_000.wav");
    let v = json(
        &home,
        &["similar", "--file", file.to_str().expect("utf-8"), "--json"],
    );
    let near = hits(&v);
    assert_eq!(near[0].0, "oneshots/hit_000.wav");
    assert!(near[0].1 < 1e-4, "{near:?}");
    assert!(v["seed"]["file_id"].is_null());

    // Library filter: only tonal notes.
    let lib = tonal.to_str().expect("utf-8");
    let v = json(&home, &["similar", &kick, "--library", lib, "--json"]);
    let near = hits(&v);
    assert!(!near.is_empty());
    assert!(
        near.iter().all(|(p, _)| p.starts_with("tonal/")),
        "{near:?}"
    );

    // Text output with weights.
    let text = ok(&sonar(
        &home,
        &["similar", &kick, "--weights", "pitch=1", "--limit", "2"],
    ));
    assert!(text.starts_with(&format!("similar to #{kick} ")), "{text}");
    assert!(
        text.contains("spectrum=0,timbre=0,pitch=1,amplitude=0"),
        "{text}"
    );
    assert!(
        text.contains(&format!("2 of {} candidates", ids.len() - 1)),
        "{text}"
    );

    let out = sonar(&home, &["similar", "999999"]);
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("no aspect vectors"),
        "{out:?}"
    );
}
