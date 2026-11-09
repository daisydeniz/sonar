//! `sonar meta <file>...`: basic metadata (duration, format, sample peak,
//! BS.1770 integrated loudness) of each file, as the pipeline computes it.

use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use sonar_core::decode::{DecodeOptions, Decoder};
use sonar_core::meta::{BasicMeta, MetaOptions};

/// Measures every file with one reused decoder; prints one line per file
/// (or a JSON array). A failing file is reported (stderr in text mode, an
/// `error` entry in JSON) and the rest still run; the command then fails.
pub fn run(files: &[PathBuf], max_meta_secs: f64, json: bool) -> Result<()> {
    let decode = DecodeOptions {
        max_analysis_secs: Some(0.0),
        ..DecodeOptions::default()
    };
    let meta = MetaOptions { max_meta_secs };
    let mut decoder = Decoder::new();
    let mut failed = 0usize;
    let mut out = Vec::new();
    for file in files {
        let result = decoder
            .decode_with_meta(file, &decode, &meta)
            .map(|(_, m)| m);
        if result.is_err() {
            failed += 1;
        }
        if json {
            out.push(to_json(file, &result));
        } else {
            match &result {
                Ok(m) => println!("{}", line(file, m)),
                Err(e) => eprintln!("{}: error: {e}", file.display()),
            }
        }
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&out)?);
    }
    if failed > 0 {
        bail!("{failed} of {} files could not be read", files.len());
    }
    Ok(())
}

fn to_json(file: &Path, r: &sonar_core::Result<BasicMeta>) -> serde_json::Value {
    let path = file.display().to_string();
    match r {
        Ok(m) => serde_json::json!({ "path": path, "meta": m }),
        Err(e) => serde_json::json!({ "path": path, "error": e.to_string() }),
    }
}

/// `path: 2.000 s, 48000 Hz, 2 ch, 24-bit, peak -6.02 dBFS, -9.03 LUFS`.
fn line(file: &Path, m: &BasicMeta) -> String {
    let bits = m
        .bit_depth
        .map_or_else(|| "lossy".to_owned(), |b| format!("{b}-bit"));
    let peak = m
        .peak_dbfs
        .map_or_else(|| "silent".to_owned(), |p| format!("{p:.2} dBFS"));
    let lufs = m
        .loudness_lufs
        .map_or_else(|| "-- LUFS".to_owned(), |l| format!("{l:.2} LUFS"));
    let secs = m.num_frames as f64 / f64::from(m.sample_rate);
    let mut s = format!(
        "{}: {:.3} s, {} Hz, {} ch, {bits}, peak {peak}, {lufs}",
        file.display(),
        m.duration_ms as f64 / 1e3,
        m.sample_rate,
        m.channels
    );
    if m.measured_s + 1e-9 < secs {
        s.push_str(&format!(" (first {:.1} s measured)", m.measured_s));
    }
    if m.partial {
        s.push_str(" [partial]");
    }
    s
}
