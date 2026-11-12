//! `sonar embed`, `sonar embed-text`, `sonar analyze --stages embed` and
//! `sonar bench embed` (T5.2). Tests that need the CLAP model files print a
//! note and pass when they are absent (`scripts/fetch-models.sh`).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use sonar_core::embed::EmbedOptions;

struct Env {
    tmp: tempfile::TempDir,
    home: PathBuf,
}

impl Env {
    fn new() -> Self {
        let tmp = tempfile::tempdir().expect("tempdir");
        Self {
            home: tmp.path().join("home"),
            tmp,
        }
    }

    fn sonar(&self, args: &[&str]) -> Output {
        self.sonar_env(args, None)
    }

    fn sonar_env(&self, args: &[&str], models: Option<&Path>) -> Output {
        let mut c = Command::new(env!("CARGO_BIN_EXE_sonar"));
        c.args(args)
            .env("SONAR_HOME", &self.home)
            .env_remove("RUST_LOG")
            .env_remove("SONAR_EMBED_EP");
        if let Some(m) = models {
            c.env("SONAR_MODELS_DIR", m);
        }
        c.output().expect("spawn sonar")
    }

    /// Fixtures (without the long file) in `<tmp>/fx`.
    fn fixtures(&self) -> PathBuf {
        let dir = self.tmp.path().join("fx");
        let out = self.sonar(&["fixtures", dir.to_str().expect("utf-8"), "--skip-long"]);
        assert!(out.status.success(), "fixtures failed: {out:?}");
        dir
    }
}

fn models_present() -> bool {
    match EmbedOptions::default().unavailable_reason() {
        None => true,
        Some(why) => {
            eprintln!("skipped: {why}");
            false
        }
    }
}

fn json(out: &Output) -> serde_json::Value {
    assert!(out.status.success(), "command failed: {out:?}");
    serde_json::from_slice(&out.stdout).expect("json")
}

#[test]
fn embed_text_prints_tokens_dims_and_unit_norm() {
    if !models_present() {
        return;
    }
    let env = Env::new();
    let v = json(&env.sonar(&["embed-text", "a", "kick", "drum", "--json"]));
    assert_eq!(v["tokens"], serde_json::json!([0, 102, 3151, 12638, 2]));
    assert_eq!(v["dims"], 512);
    assert!((v["norm"].as_f64().expect("norm") - 1.0).abs() < 1e-3);
    assert!(v.get("embedding").is_none());
    let full = json(&env.sonar(&["embed-text", "snare", "--json", "--full"]));
    assert_eq!(full["embedding"].as_array().map(Vec::len), Some(512));

    let text = env.sonar(&["embed-text", "a kick drum"]);
    assert!(text.status.success());
    let s = String::from_utf8_lossy(&text.stdout);
    assert!(s.contains("512 dims") && s.contains("5 tokens"), "{s}");

    assert!(!env.sonar(&["embed-text", "   "]).status.success());
}

#[test]
fn embed_files_reports_dims_norm_timing_and_fails_on_bad_files() {
    if !models_present() {
        return;
    }
    let env = Env::new();
    let fx = env.fixtures();
    // The first two fixture WAVs (some fixtures are deliberately corrupt,
    // so pick from a folder of intact one-shots).
    let wavs: Vec<PathBuf> = walk_wavs(&fx)
        .into_iter()
        .filter(|p| p.components().any(|c| c.as_os_str() == "tonal"))
        .collect();
    let files: Vec<&str> = wavs
        .iter()
        .take(2)
        .map(|p| p.to_str().expect("utf-8"))
        .collect();
    assert_eq!(files.len(), 2, "no fixture wavs under {}", fx.display());

    let mut args = vec!["embed", "--json", "--full"];
    args.extend(&files);
    let v = json(&env.sonar(&args));
    let rows = v.as_array().expect("array");
    assert_eq!(rows.len(), 2);
    for r in rows {
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(r["dims"], 512);
        assert!((r["norm"].as_f64().expect("norm") - 1.0).abs() < 1e-3);
        assert_eq!(r["embedding"].as_array().map(Vec::len), Some(512));
        assert!(r["prep_ms"].as_f64().is_some() && r["model_ms"].as_f64().is_some());
    }

    let bad = env.tmp.path().join("bad.wav");
    fs::write(&bad, b"not audio").expect("write");
    let out = env.sonar(&["embed", files[0], bad.to_str().expect("utf-8")]);
    assert!(!out.status.success(), "a bad file must fail the command");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("512 dims"),
        "good file still reported: {stdout}"
    );
    assert!(String::from_utf8_lossy(&out.stderr).contains("bad.wav"));
}

fn walk_wavs(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "wav") {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

#[test]
fn missing_model_is_an_error_for_embed_and_a_skip_for_analyze() {
    let env = Env::new();
    let empty = env.tmp.path().join("no-models");
    fs::create_dir(&empty).expect("mkdir");
    let wav = env.tmp.path().join("x.wav");
    fs::write(&wav, b"RIFF").expect("write");

    let out = env.sonar_env(&["embed", wav.to_str().expect("utf-8")], Some(&empty));
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("not found") || err.contains("CLAP"), "{err}");
    let out = env.sonar_env(&["embed-text", "kick"], Some(&empty));
    assert!(!out.status.success());

    // analyze: skipped cleanly, nothing recorded per file, exit code 0.
    let lib = env.fixtures();
    let scan = env.sonar(&["scan", lib.to_str().expect("utf-8")]);
    assert!(scan.status.success(), "{scan:?}");
    let out = env.sonar_env(&["analyze", "--stages", "embed", "--json"], Some(&empty));
    let v = json(&out);
    assert_eq!(v["stages"].as_array().map(Vec::len), Some(0), "{v}");
    assert_eq!(v["skipped"][0]["stage"], "embed");
    let conn = rusqlite::Connection::open(env.home.join("sonar.db")).expect("db");
    let n: i64 = conn
        .query_row(
            "SELECT count(*) FROM file_stages WHERE stage='embed'",
            [],
            |r| r.get(0),
        )
        .expect("count");
    assert_eq!(n, 0);
}

#[test]
fn analyze_embed_stage_embeds_the_library_and_resumes() {
    if !models_present() {
        return;
    }
    let env = Env::new();
    let lib = env.fixtures();
    let scan = env.sonar(&["scan", lib.to_str().expect("utf-8")]);
    assert!(scan.status.success(), "{scan:?}");

    let v = json(&env.sonar(&["analyze", "--stages", "embed", "--json", "--sessions", "4"]));
    let stage = &v["stages"][0];
    assert_eq!(stage["stage"], "embed");
    let (total, ok, errors) = (
        stage["total"].as_u64().expect("total"),
        stage["ok"].as_u64().expect("ok") + stage["partial"].as_u64().expect("partial"),
        stage["errors"].as_u64().expect("errors"),
    );
    assert!(ok > 50 && ok + errors == total, "{stage}");
    assert!(env
        .home
        .join("index")
        .join("clap")
        .join("emb.f32")
        .is_file());

    // Second run: nothing pending.
    let again = json(&env.sonar(&["analyze", "--stages", "embed", "--json", "--sessions", "4"]));
    assert_eq!(again["stages"][0]["total"], 0, "{again}");
    let conn = rusqlite::Connection::open(env.home.join("sonar.db")).expect("db");
    let rows: i64 = conn
        .query_row(
            "SELECT count(*) FROM file_stages WHERE stage='embed' AND outcome IN ('ok','partial')",
            [],
            |r| r.get(0),
        )
        .expect("count");
    assert_eq!(rows as u64, ok);
}

#[test]
fn bench_embed_reports_model_front_end_and_end_to_end() {
    if !models_present() {
        return;
    }
    let env = Env::new();
    let v = json(&env.sonar(&[
        "bench",
        "embed",
        "--json",
        "--files",
        "60",
        "--model-clips",
        "30",
        "--sessions",
        "2",
    ]));
    assert_eq!(v["sessions"], 2);
    assert!(v["model"]["clips_per_sec"].as_f64().expect("rate") > 0.0);
    assert!(v["front_ms_per_file"].as_f64().expect("front") > 0.0);
    let stage = &v["end_to_end"]["stages"][0];
    assert_eq!(
        (stage["stage"].as_str(), stage["total"].as_u64()),
        (Some("embed"), Some(60))
    );
}
