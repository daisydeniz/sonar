//! `sonar aspects <file>...`: raw similarity aspect vectors of each file's
//! analysis window and, with ≥ 2 files, vectors normalized with stats over
//! the given files.

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{bail, Context, Result};
use sonar_core::decode::{DecodeOptions, Decoder, ANALYSIS_RATE};
use sonar_core::dsp::aspects::{
    amplitude, pitch, Aspect, AspectExtractor, AspectVectors, LibraryNorm, NormalizedAspects,
    ASPECTS_VERSION,
};
use sonar_core::dsp::Analyzer;

/// One analysed file.
struct Done {
    secs: f64,
    vectors: AspectVectors,
    ms: f64,
}

/// Decodes and analyzes every file with one reused decoder, analyzer and
/// extractor. A failing file is reported (stderr in text mode, an `error`
/// entry in JSON) and the rest still run; the command then fails.
pub fn run(files: &[PathBuf], json: bool) -> Result<()> {
    let opts = DecodeOptions::default();
    let mut decoder = Decoder::new();
    let mut analyzer = Analyzer::new(ANALYSIS_RATE).context("building the DSP analyzer")?;
    let mut extractor =
        AspectExtractor::new(ANALYSIS_RATE).context("building the aspect extractor")?;
    let results: Vec<sonar_core::Result<Done>> = files
        .iter()
        .map(|f| extract(&mut decoder, &mut analyzer, &mut extractor, f, &opts))
        .collect();
    let failed = results.iter().filter(|r| r.is_err()).count();

    let mut lib = LibraryNorm::new();
    for done in results.iter().flatten() {
        lib.push(&done.vectors).context("accumulating stats")?;
    }
    let normalized: Vec<Option<NormalizedAspects>> = results
        .iter()
        .map(|r| match r {
            Ok(d) if lib.count() >= 2 => Some(lib.normalize(&d.vectors)),
            _ => None,
        })
        .collect();

    if json {
        let out: Vec<_> = files
            .iter()
            .zip(&results)
            .zip(&normalized)
            .map(|((f, r), n)| to_json(f, r, n.as_ref()))
            .collect();
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        for (f, r) in files.iter().zip(&results) {
            match r {
                Ok(d) => print_text(f, d),
                Err(e) => eprintln!("{}: error: {e}", f.display()),
            }
        }
        print_distances(files, &normalized, lib.count());
    }
    if failed > 0 {
        bail!("{failed} of {} files could not be analyzed", files.len());
    }
    Ok(())
}

fn extract(
    decoder: &mut Decoder,
    analyzer: &mut Analyzer,
    extractor: &mut AspectExtractor,
    file: &Path,
    opts: &DecodeOptions,
) -> sonar_core::Result<Done> {
    let decoded = decoder.decode(file, opts)?;
    let started = Instant::now();
    let features = analyzer.analyze(&decoded.samples)?;
    let vectors = extractor.extract(&decoded.samples, &features)?;
    Ok(Done {
        secs: decoded.analysis_secs(),
        vectors,
        ms: started.elapsed().as_secs_f64() * 1e3,
    })
}

fn to_json(
    file: &Path,
    r: &sonar_core::Result<Done>,
    n: Option<&NormalizedAspects>,
) -> serde_json::Value {
    let path = file.display().to_string();
    let d = match r {
        Ok(d) => d,
        Err(e) => return serde_json::json!({ "path": path, "error": e.to_string() }),
    };
    let raw: serde_json::Map<String, serde_json::Value> = d
        .vectors
        .parts()
        .iter()
        .map(|(a, v)| (a.name().to_string(), serde_json::json!(v)))
        .collect();
    let mut v = serde_json::json!({
        "path": path,
        "aspects_version": ASPECTS_VERSION,
        "analysis_secs": d.secs,
        "raw": raw,
        "ms": d.ms,
    });
    if let Some(n) = n {
        let norm: serde_json::Map<String, serde_json::Value> = Aspect::ALL
            .iter()
            .map(|a| (a.name().to_string(), serde_json::json!(n.get(*a))))
            .collect();
        v["normalized"] = serde_json::Value::Object(norm);
    }
    v
}

fn print_text(file: &Path, d: &Done) {
    let dims: Vec<String> = d
        .vectors
        .parts()
        .iter()
        .map(|(a, v)| format!("{} {}", a.name(), v.len()))
        .collect();
    let p = &d.vectors.pitch;
    let a = &d.vectors.amplitude;
    let f0 = if p[pitch::VOICED] == 0.0 {
        "unvoiced".to_string()
    } else {
        format!("{:.1} Hz", 440.0 * 2f32.powf(p[pitch::F0]))
    };
    println!(
        "{}: {:.3} s, {} dims; f0 {} (voiced {:.2}), loudness {:.1} dBFS, attack {:.1} ms, {:.1} ms",
        file.display(),
        d.secs,
        dims.join(", "),
        f0,
        p[pitch::VOICED],
        a[amplitude::LOUDNESS],
        1e3 * 10f32.powf(a[amplitude::ATTACK]),
        d.ms
    );
}

/// L2 distance of every normalized file to the first one, per aspect.
fn print_distances(files: &[PathBuf], normalized: &[Option<NormalizedAspects>], n: u64) {
    let Some((first_i, first)) = normalized
        .iter()
        .enumerate()
        .find_map(|(i, v)| v.as_ref().map(|v| (i, v)))
    else {
        return;
    };
    println!(
        "normalized over {n} files (overall {} dims); L2 distance to {}:",
        Aspect::Overall.dim(),
        files[first_i].display()
    );
    for (f, v) in files.iter().zip(normalized) {
        let Some(v) = v else { continue };
        let parts: Vec<String> = Aspect::ALL
            .iter()
            .map(|a| {
                let d: f32 = v
                    .get(*a)
                    .iter()
                    .zip(first.get(*a))
                    .map(|(x, y)| (x - y) * (x - y))
                    .sum();
                format!("{} {:.3}", a.name(), d.sqrt())
            })
            .collect();
        println!("  {}: {}", f.display(), parts.join(", "));
    }
}
