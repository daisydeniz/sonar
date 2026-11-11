//! `sonar key <file>...`: musical key (chroma + key profiles, filename
//! prior) and, for pitched one-shots, the note (YIN), as the `dsp` stage
//! computes them (ADR-022).

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{bail, Context, Result};
use clap::Args;
use sonar_core::classify::{classify_kind, kind_hint, KindResult};
use sonar_core::decode::{DecodeOptions, Decoder, ANALYSIS_RATE};
use sonar_core::dsp::Analyzer;
use sonar_core::key::{key_and_note, tonal_chroma, KeyNote, NoteTracker, KEY_VERSION};
use sonar_core::text::filename_prior;

/// Arguments of `sonar key`.
#[derive(Debug, Args)]
pub struct KeyArgs {
    /// Audio files (wav, aif/aiff, flac, mp3, ogg, m4a).
    #[arg(required = true)]
    pub files: Vec<PathBuf>,
    /// Print a JSON array instead of text.
    #[arg(long)]
    pub json: bool,
    /// Ignore file and folder names (audio evidence only, as the eval).
    #[arg(long)]
    pub no_names: bool,
}

/// One analysed file.
struct Done {
    kind: KindResult,
    key_note: KeyNote,
    /// Share of the band power in sinusoidal peaks (the tonality gate).
    tonal_ratio: f32,
    /// Key + note time on top of decode, DSP features and classification (ms).
    key_ms: f64,
}

/// Decodes, analyzes and classifies every file with one reused decoder,
/// analyzer and note tracker, then estimates key and note. A failing file
/// is reported and the rest still run; the command then fails.
pub fn run(args: &KeyArgs) -> Result<()> {
    let opts = DecodeOptions::default();
    let mut decoder = Decoder::new();
    let mut analyzer = Analyzer::new(ANALYSIS_RATE).context("building the DSP analyzer")?;
    let mut tracker = NoteTracker::new(ANALYSIS_RATE).context("building the note tracker")?;
    let mut failed = 0usize;
    let mut out = Vec::new();
    for file in &args.files {
        let result = analyze(
            &mut decoder,
            &mut analyzer,
            &mut tracker,
            file,
            &opts,
            !args.no_names,
        );
        if result.is_err() {
            failed += 1;
        }
        if args.json {
            out.push(to_json(file, &result));
        } else {
            match &result {
                Ok(d) => print_text(file, d),
                Err(e) => eprintln!("{}: error: {e}", file.display()),
            }
        }
    }
    if args.json {
        println!("{}", serde_json::to_string_pretty(&out)?);
    }
    if failed > 0 {
        bail!(
            "{failed} of {} files could not be analysed",
            args.files.len()
        );
    }
    Ok(())
}

fn analyze(
    decoder: &mut Decoder,
    analyzer: &mut Analyzer,
    tracker: &mut NoteTracker,
    file: &Path,
    opts: &DecodeOptions,
    use_names: bool,
) -> sonar_core::Result<Done> {
    let decoded = decoder.decode(file, opts)?;
    let features = analyzer.analyze(&decoded.samples)?;
    let (hint, prior) = if use_names {
        (kind_hint(file), filename_prior(file).key)
    } else {
        (None, None)
    };
    let kind = classify_kind(&features, decoded.source.duration_s, hint);
    let started = Instant::now();
    let key_note = key_and_note(&kind, &features, &decoded.samples, tracker, prior)?;
    let key_ms = started.elapsed().as_secs_f64() * 1e3;
    Ok(Done {
        kind,
        key_note,
        tonal_ratio: tonal_chroma(&features.spectrogram).tonal_ratio,
        key_ms,
    })
}

fn to_json(file: &Path, r: &sonar_core::Result<Done>) -> serde_json::Value {
    let path = file.display().to_string();
    match r {
        Ok(d) => serde_json::json!({
            "path": path,
            "key_version": KEY_VERSION,
            "kind": d.kind.kind,
            "key": d.key_note.key.map(|k| k.key.to_string()),
            "key_confidence": d.key_note.key.map(|k| k.confidence),
            "key_source": d.key_note.key.map(|k| k.source),
            "note": d.key_note.note,
            "note_name": d.key_note.note.map(|n| n.name()),
            "tonal_ratio": d.tonal_ratio,
            "key_ms": d.key_ms,
        }),
        Err(e) => serde_json::json!({ "path": path, "error": e.to_string() }),
    }
}

fn print_text(file: &Path, d: &Done) {
    let key = d.key_note.key.map_or_else(
        || "no key".to_owned(),
        |k| {
            format!(
                "key {} (confidence {:.2}, {})",
                k.key,
                k.confidence,
                k.source.as_str()
            )
        },
    );
    let note = d.key_note.note.map_or_else(String::new, |n| {
        format!(
            ", note {} = MIDI {} {:+.0} ct (confidence {:.2})",
            n.name(),
            n.midi,
            n.cents,
            n.confidence
        )
    });
    println!(
        "{}: {:?} {key}{note} tonal {:.2} [{:.2} ms]",
        file.display(),
        d.kind.kind,
        d.tonal_ratio,
        d.key_ms,
    );
}
