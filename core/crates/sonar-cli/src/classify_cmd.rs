//! `sonar classify <file>...`: loop vs one-shot decision of each file
//! (analysis window + true duration + optional filename hint) with the
//! heuristic scores behind it.

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{bail, Context, Result};
use sonar_core::classify::{classify_kind, kind_hint, KindResult, KIND_VERSION};
use sonar_core::decode::{DecodeOptions, Decoder, ANALYSIS_RATE};
use sonar_core::dsp::Analyzer;

/// One classified file.
struct Done {
    result: KindResult,
    /// Classifier time on top of decode + DSP features (ms).
    classify_ms: f64,
}

/// Decodes, analyzes and classifies every file with one reused decoder and
/// analyzer. `use_names: false` hides file names from the classifier (as
/// the eval does). A failing file is reported (stderr in text mode, an
/// `error` entry in JSON) and the rest still run; the command then fails.
pub fn run(files: &[PathBuf], json: bool, use_names: bool) -> Result<()> {
    let opts = DecodeOptions::default();
    let mut decoder = Decoder::new();
    let mut analyzer = Analyzer::new(ANALYSIS_RATE).context("building the DSP analyzer")?;
    let mut failed = 0usize;
    let mut out = Vec::new();
    for file in files {
        let result = classify(&mut decoder, &mut analyzer, file, &opts, use_names);
        if result.is_err() {
            failed += 1;
        }
        if json {
            out.push(to_json(file, &result));
        } else {
            match &result {
                Ok(d) => print_text(file, d),
                Err(e) => eprintln!("{}: error: {e}", file.display()),
            }
        }
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&out)?);
    }
    if failed > 0 {
        bail!("{failed} of {} files could not be classified", files.len());
    }
    Ok(())
}

fn classify(
    decoder: &mut Decoder,
    analyzer: &mut Analyzer,
    file: &Path,
    opts: &DecodeOptions,
    use_names: bool,
) -> sonar_core::Result<Done> {
    let decoded = decoder.decode(file, opts)?;
    let features = analyzer.analyze(&decoded.samples)?;
    let started = Instant::now();
    let hint = if use_names { kind_hint(file) } else { None };
    let result = classify_kind(&features, decoded.source.duration_s, hint);
    let classify_ms = started.elapsed().as_secs_f64() * 1e3;
    Ok(Done {
        result,
        classify_ms,
    })
}

fn to_json(file: &Path, r: &sonar_core::Result<Done>) -> serde_json::Value {
    let path = file.display().to_string();
    match r {
        Ok(d) => serde_json::json!({
            "path": path,
            "kind_version": KIND_VERSION,
            "result": d.result,
            "classify_ms": d.classify_ms,
        }),
        Err(e) => serde_json::json!({ "path": path, "error": e.to_string() }),
    }
}

fn print_text(file: &Path, d: &Done) {
    let r = &d.result;
    let s = &r.scores;
    let tail = s
        .tail_db
        .map_or_else(|| "unknown".to_owned(), |t| format!("{t:.1} dB"));
    let hint = r.hint.map_or_else(|| "-".to_owned(), |h| format!("{h:?}"));
    println!(
        "{}: {:?} (confidence {:.2}, p_loop {:.3}, {:?}, hint {hint})",
        file.display(),
        r.kind,
        r.confidence,
        r.loop_probability,
        r.source,
    );
    println!(
        "  periodicity {:.2} @ {:.2} s, pulse {:.2} dB, onsets {}, duration {:.2} s{}, tail {tail}, audio logit {:+.2}, {:.2} ms",
        s.periodicity,
        s.period_s,
        s.pulse_db,
        s.onsets,
        s.duration_s,
        if s.truncated { " (truncated)" } else { "" },
        r.audio_logit,
        d.classify_ms,
    );
}
