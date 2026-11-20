//! `sonar tags list|rename|merge|delete|similar|ignore` end to end in a
//! temp SONAR_HOME (T12.2, ADR-043).

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

fn json(s: &str) -> serde_json::Value {
    serde_json::from_str(s).expect("json")
}

#[test]
fn tags_management_commands() {
    let tmp = tempfile::tempdir().expect("tmp");
    let lib = tmp.path().join("lib");
    fs::create_dir_all(&lib).expect("mkdir");
    for name in ["a.wav", "b.wav", "c.wav"] {
        fs::write(lib.join(name), b"x").expect("write");
    }
    let home = tmp.path().join("home");
    ok(&sonar(&home, &["scan", lib.to_str().expect("utf-8")]));
    ok(&sonar(&home, &["tag", "add", "  Dark   Vibe ", "1", "2"]));
    ok(&sonar(&home, &["tag", "add", "groove", "1"]));
    ok(&sonar(&home, &["tag", "add", "grove", "3"]));
    let v = json(&ok(&sonar(&home, &["tags", "list", "--json"])));
    assert_eq!(v[0]["name"], "Dark Vibe");
    assert_eq!(v[0]["files"], 2);

    let v = json(&ok(&sonar(&home, &["tags", "similar", "--json"])));
    assert_eq!(v.as_array().expect("array").len(), 1);
    assert_eq!(v[0]["reason"], "typo");
    ok(&sonar(&home, &["tags", "ignore", "grove", "GROOVE"]));
    let v = json(&ok(&sonar(&home, &["tags", "similar", "--json"])));
    assert!(v.as_array().expect("array").is_empty());

    let rename = ["tags", "rename", "dark vibe", "Moody", "--json"];
    assert_eq!(json(&ok(&sonar(&home, &rename)))["files"], json("[1,2]"));
    let merge = [
        "tags", "merge", "--into", "moody", "groove", "grove", "--json",
    ];
    assert_eq!(json(&ok(&sonar(&home, &merge)))["files"], json("[1,3]"));
    assert!(ok(&sonar(&home, &["tags", "list"])).contains("3  Moody"));
    let bad = sonar(&home, &["tags", "delete", "nope"]);
    assert!(!bad.status.success());
    assert!(String::from_utf8_lossy(&bad.stderr).contains("no such tag"));
    let del = ["tags", "delete", "MOODY", "--json"];
    assert_eq!(json(&ok(&sonar(&home, &del)))["files"], json("[1,2,3]"));
    assert!(ok(&sonar(&home, &["tags", "list"])).trim().is_empty());
}
