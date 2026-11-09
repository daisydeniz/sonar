//! `sonar decode <file>`: decode like the analysis pipeline and report.

use std::path::Path;
use std::time::Instant;

use anyhow::{Context, Result};
use sonar_core::decode::{decode_file, DecodeOptions, Decoded};

/// Decodes `file` and prints a text (or JSON) summary to stdout.
pub fn run(file: &Path, rate: u32, max_secs: Option<f64>, json: bool) -> Result<()> {
    let opts = DecodeOptions {
        target_rate: rate,
        max_analysis_secs: max_secs,
        cancel: None,
    };
    let started = Instant::now();
    let d = decode_file(file, &opts).with_context(|| format!("decoding {}", file.display()))?;
    let elapsed = started.elapsed();
    if json {
        println!("{}", serde_json::to_string_pretty(&to_json(file, &d))?);
    } else {
        print_text(file, &d, elapsed.as_secs_f64());
    }
    Ok(())
}

fn to_json(file: &Path, d: &Decoded) -> serde_json::Value {
    serde_json::json!({
        "path": file.display().to_string(),
        "source": d.source,
        "analysis": {
            "sample_rate": d.sample_rate,
            "samples": d.samples.len(),
            "secs": d.analysis_secs(),
            "peak": peak(d),
        },
        "truncated_for_analysis": d.truncated_for_analysis,
        "partial": d.partial,
        "problem": d.problem,
    })
}

fn print_text(file: &Path, d: &Decoded, secs: f64) {
    let s = &d.source;
    let bits = s
        .bit_depth
        .map_or_else(|| "lossy".to_owned(), |b| format!("{b}-bit"));
    let declared = s
        .declared_frames
        .map_or_else(|| "none".to_owned(), |n| n.to_string());
    println!("file: {}", file.display());
    println!("format: {} / {}", s.container, s.codec);
    println!("source: {} Hz, {} ch, {bits}", s.sample_rate, s.channels);
    println!(
        "duration: {:.6} s ({} frames; header: {declared})",
        s.duration_s, s.num_frames
    );
    println!(
        "analysis: {} samples @ {} Hz ({:.3} s), peak {:.4}{}",
        d.samples.len(),
        d.sample_rate,
        d.analysis_secs(),
        peak(d),
        if d.truncated_for_analysis {
            ", truncated to window"
        } else {
            ""
        }
    );
    match &d.problem {
        Some(p) if d.partial => println!("partial: yes ({p})"),
        _ => println!("partial: no"),
    }
    println!("decode time: {:.1} ms", secs * 1e3);
}

fn peak(d: &Decoded) -> f32 {
    d.samples.iter().fold(0.0f32, |m, x| m.max(x.abs()))
}
