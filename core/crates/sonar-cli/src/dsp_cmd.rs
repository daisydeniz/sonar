//! `sonar dsp <file>...`: frame-level DSP features (STFT, log-mel, MFCC,
//! onset envelope) of each file's analysis window, summarized.

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{bail, Context, Result};
use sonar_core::decode::{DecodeOptions, Decoder, ANALYSIS_RATE};
use sonar_core::dsp::{Analyzer, Features, DSP_VERSION};

/// Per-file summary printed by the command.
struct Summary {
    secs: f64,
    frames: usize,
    frame_rate: f32,
    mfcc_mean: Vec<f32>,
    onset_mean: f32,
    onset_max: f32,
    dsp_ms: f64,
}

/// Decodes and analyzes every file with one reused decoder and analyzer.
/// A failing file is reported (stderr in text mode, an `error` entry in
/// JSON) and the rest still run; the command then fails.
pub fn run(files: &[PathBuf], json: bool) -> Result<()> {
    let opts = DecodeOptions::default();
    let mut decoder = Decoder::new();
    let mut analyzer = Analyzer::new(ANALYSIS_RATE).context("building the DSP analyzer")?;
    let mut failed = 0usize;
    let mut out = Vec::new();
    for file in files {
        let result = summarize(&mut decoder, &mut analyzer, file, &opts);
        if result.is_err() {
            failed += 1;
        }
        if json {
            out.push(to_json(file, &result));
        } else {
            match &result {
                Ok(s) => print_text(file, s),
                Err(e) => eprintln!("{}: error: {e}", file.display()),
            }
        }
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&out)?);
    }
    if failed > 0 {
        bail!("{failed} of {} files could not be analyzed", files.len());
    }
    Ok(())
}

fn summarize(
    decoder: &mut Decoder,
    analyzer: &mut Analyzer,
    file: &Path,
    opts: &DecodeOptions,
) -> sonar_core::Result<Summary> {
    let decoded = decoder.decode(file, opts)?;
    let started = Instant::now();
    let f = analyzer.analyze(&decoded.samples)?;
    let dsp_ms = started.elapsed().as_secs_f64() * 1e3;
    Ok(summary(&f, decoded.analysis_secs(), dsp_ms))
}

fn summary(f: &Features, secs: f64, dsp_ms: f64) -> Summary {
    let (mfcc_mean, _) = f.mfcc.column_stats();
    let n = f.onset.len().max(1) as f32;
    Summary {
        secs,
        frames: f.spectrogram.n_frames,
        frame_rate: f.frame_rate(),
        mfcc_mean,
        onset_mean: f.onset.iter().sum::<f32>() / n,
        onset_max: f.onset.iter().copied().fold(0.0, f32::max),
        dsp_ms,
    }
}

fn to_json(file: &Path, r: &sonar_core::Result<Summary>) -> serde_json::Value {
    let path = file.display().to_string();
    match r {
        Ok(s) => serde_json::json!({
            "path": path,
            "dsp_version": DSP_VERSION,
            "analysis_secs": s.secs,
            "frames": s.frames,
            "frame_rate": s.frame_rate,
            "mfcc_mean": s.mfcc_mean,
            "onset": { "len": s.frames, "mean": s.onset_mean, "max": s.onset_max },
            "dsp_ms": s.dsp_ms,
        }),
        Err(e) => serde_json::json!({ "path": path, "error": e.to_string() }),
    }
}

fn print_text(file: &Path, s: &Summary) {
    let mfcc: Vec<String> = s.mfcc_mean.iter().map(|c| format!("{c:.1}")).collect();
    println!(
        "{}: {:.3} s, {} frames @ {:.2}/s, onset mean {:.3} max {:.3}, dsp {:.1} ms",
        file.display(),
        s.secs,
        s.frames,
        s.frame_rate,
        s.onset_mean,
        s.onset_max,
        s.dsp_ms
    );
    println!("  mfcc mean: [{}]", mfcc.join(", "));
}
