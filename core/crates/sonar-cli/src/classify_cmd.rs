//! `sonar classify <file>...`: loop vs one-shot decision and category of
//! each file (analysis window + true duration + optional filename hint;
//! category also from CLAP zero-shot when the model is available) with the
//! scores behind them.

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{bail, Context, Result};
use sonar_core::classify::category::clap::ClapScorer;
use sonar_core::classify::{
    category_hint, classify_category, classify_kind, kind_hint, CategoryResult, DspCues,
    KindResult, CATEGORY_VERSION, KIND_VERSION,
};
use sonar_core::decode::{DecodeOptions, Decoder, ANALYSIS_RATE};
use sonar_core::dsp::aspects::AspectExtractor;
use sonar_core::dsp::Analyzer;
use sonar_core::store::analysis::KindTempo;

use crate::eval_cmd::load_clap;

/// One classified file.
struct Done {
    result: KindResult,
    category: CategoryResult,
    cues: DspCues,
    /// Classifier time on top of decode + DSP features + aspects + CLAP (ms).
    classify_ms: f64,
}

/// Reused per-run state: decoder, analyzer, aspect extractor, CLAP.
struct Tools {
    decoder: Decoder,
    analyzer: Analyzer,
    extractor: AspectExtractor,
    clap: Option<std::sync::Arc<ClapScorer>>,
    use_names: bool,
}

/// Decodes, analyzes and classifies every file with one reused decoder,
/// analyzer and (when `use_clap` and the model is available) CLAP session.
/// `use_names: false` hides file names from the classifiers (as the eval
/// does). A failing file is reported (stderr in text mode, an `error` entry
/// in JSON) and the rest still run; the command then fails.
pub fn run(files: &[PathBuf], json: bool, use_names: bool, use_clap: bool) -> Result<()> {
    let opts = DecodeOptions::default();
    let mut tools = Tools {
        decoder: Decoder::new(),
        analyzer: Analyzer::new(ANALYSIS_RATE).context("building the DSP analyzer")?,
        extractor: AspectExtractor::new(ANALYSIS_RATE).context("building the aspect extractor")?,
        clap: if use_clap { load_clap() } else { None },
        use_names,
    };
    let mut failed = 0usize;
    let mut out = Vec::new();
    for file in files {
        let result = classify(&mut tools, file, &opts);
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

fn classify(t: &mut Tools, file: &Path, opts: &DecodeOptions) -> sonar_core::Result<Done> {
    let decoded = t.decoder.decode(file, opts)?;
    let features = t.analyzer.analyze(&decoded.samples)?;
    let aspects = t.extractor.extract(&decoded.samples, &features)?;
    let clap = match &t.clap {
        Some(c) => Some(c.scores(file)?),
        None => None,
    };
    let started = Instant::now();
    let duration = decoded.source.duration_s;
    let (k_hint, c_hint) = if t.use_names {
        (kind_hint(file), category_hint(file))
    } else {
        (None, None)
    };
    let result = classify_kind(&features, duration, k_hint);
    // The same inputs the job pipeline stores and reads back.
    let stored = KindTempo::new(&result, None);
    let cues = DspCues::new(&aspects, stored.kind, stored.kind_conf, duration);
    let category = classify_category(&cues, clap.as_ref(), c_hint);
    let classify_ms = started.elapsed().as_secs_f64() * 1e3;
    Ok(Done {
        result,
        category,
        cues,
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
            "category_version": CATEGORY_VERSION,
            "category": d.category,
            "category_top3": d.category.top3().map(|(c, p)| (c.as_str(), p)),
            "cues": d.cues,
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
    let c = &d.category;
    let name = |x: Option<sonar_core::classify::Category>| x.map_or("-", |x| x.as_str());
    let top3: Vec<String> = c
        .top3()
        .iter()
        .map(|(k, p)| format!("{} {p:.2}", k.as_str()))
        .collect();
    println!(
        "  category {} (confidence {:.2}, {:?}, {}, hint {}, audio {}; top {})",
        name(c.category),
        c.confidence,
        c.source,
        if c.clap { "CLAP + DSP" } else { "DSP" },
        name(c.hint),
        name(c.audio),
        top3.join(", "),
    );
}
