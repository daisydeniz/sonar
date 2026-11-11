//! `sonar bpm <file>...`: loop/one-shot decision and, for loops, the tempo
//! (detected, or the filename BPM when detection is weak).

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{bail, Context, Result};
use sonar_core::classify::{classify_kind, kind_hint, KindResult, KIND_VERSION};
use sonar_core::decode::{DecodeOptions, Decoder, ANALYSIS_RATE};
use sonar_core::dsp::Analyzer;
use sonar_core::tempo::{loop_tempo, Tempo, TEMPO_VERSION};
use sonar_core::text::filename_prior;

/// One analysed file.
struct Done {
    kind: KindResult,
    tempo: Option<Tempo>,
    /// Tempo time on top of decode, DSP features and classification (ms).
    tempo_ms: f64,
}

/// Decodes, analyzes, classifies and estimates the tempo of every file with
/// one reused decoder and analyzer. `use_names: false` hides file names
/// (no kind hint, no BPM prior), as the eval does. A failing file is
/// reported and the rest still run; the command then fails.
pub fn run(files: &[PathBuf], json: bool, use_names: bool) -> Result<()> {
    let opts = DecodeOptions::default();
    let mut decoder = Decoder::new();
    let mut analyzer = Analyzer::new(ANALYSIS_RATE).context("building the DSP analyzer")?;
    let mut failed = 0usize;
    let mut out = Vec::new();
    for file in files {
        let result = analyze(&mut decoder, &mut analyzer, file, &opts, use_names);
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
        bail!("{failed} of {} files could not be analysed", files.len());
    }
    Ok(())
}

fn analyze(
    decoder: &mut Decoder,
    analyzer: &mut Analyzer,
    file: &Path,
    opts: &DecodeOptions,
    use_names: bool,
) -> sonar_core::Result<Done> {
    let decoded = decoder.decode(file, opts)?;
    let features = analyzer.analyze(&decoded.samples)?;
    let duration = decoded.source.duration_s;
    let (hint, prior) = if use_names {
        (kind_hint(file), filename_prior(file).bpm)
    } else {
        (None, None)
    };
    let kind = classify_kind(&features, duration, hint);
    let started = Instant::now();
    let tempo = loop_tempo(&kind, &features, duration, prior);
    let tempo_ms = started.elapsed().as_secs_f64() * 1e3;
    Ok(Done {
        kind,
        tempo,
        tempo_ms,
    })
}

fn to_json(file: &Path, r: &sonar_core::Result<Done>) -> serde_json::Value {
    let path = file.display().to_string();
    match r {
        Ok(d) => serde_json::json!({
            "path": path,
            "kind_version": KIND_VERSION,
            "tempo_version": TEMPO_VERSION,
            "kind": d.kind.kind,
            "kind_confidence": d.kind.confidence,
            "kind_source": d.kind.source,
            "tempo": d.tempo,
            "tempo_ms": d.tempo_ms,
        }),
        Err(e) => serde_json::json!({ "path": path, "error": e.to_string() }),
    }
}

fn print_text(file: &Path, d: &Done) {
    let k = &d.kind;
    let tempo = d.tempo.map_or_else(
        || "no BPM".to_owned(),
        |t| {
            let beats = t.beats.map_or_else(String::new, |b| format!(", {b} beats"));
            format!(
                "{:.2} BPM (confidence {:.2}, {}{beats})",
                t.bpm,
                t.confidence,
                t.source.as_str()
            )
        },
    );
    println!(
        "{}: {:?} ({:.2}, {:?}) {tempo} [{:.2} ms]",
        file.display(),
        k.kind,
        k.confidence,
        k.source,
        d.tempo_ms,
    );
}
