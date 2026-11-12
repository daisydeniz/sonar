//! `sonar embed`, `sonar embed-text` and `sonar bench embed`: CLAP
//! embeddings of audio files and text queries (T5.2, ADR-026, ADR-027).

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{bail, Context, Result};
use clap::Args;
use serde::Serialize;
use sonar_core::bench::{self, EmbedBench, EmbedBenchOptions};
use sonar_core::embed::{
    self, AudioPrep, EmbedEp, EmbedOptions, EmbedPool, Embedding, TextEncoder, EMBED_DIM,
};

use crate::bench_cmd::Work;

/// Model settings shared by every command that loads CLAP.
#[derive(Debug, Clone, Args, Default)]
pub struct EmbedOpts {
    /// Directory containing `clap-htsat-unfused/` (default:
    /// `$SONAR_MODELS_DIR`, the repo's `models/`, or the data dir's `models/`).
    #[arg(long, value_name = "DIR")]
    pub models_dir: Option<PathBuf>,
    /// ONNX Runtime execution provider: `cpu` (default) or `coreml`
    /// (falls back to CPU if it cannot load). Also `$SONAR_EMBED_EP`.
    #[arg(long, value_name = "EP")]
    pub ep: Option<EmbedEp>,
    /// Audio sessions (0 = one per performance core; 1 for CoreML).
    #[arg(long, default_value_t = 0)]
    pub sessions: usize,
    /// Most clips a session runs at once (0 = default).
    #[arg(long, default_value_t = 0)]
    pub max_batch: usize,
}

impl EmbedOpts {
    /// Core options for these flags.
    pub fn to_options(&self) -> EmbedOptions {
        EmbedOptions {
            models_dir: self.models_dir.clone(),
            sessions: self.sessions,
            max_batch: self.max_batch,
            ep: self.ep.unwrap_or_else(EmbedEp::from_env),
        }
    }
}

/// `sonar embed` arguments.
#[derive(Debug, Clone, Args)]
pub struct EmbedArgs {
    /// Audio files (wav, aif/aiff, flac, mp3, ogg, m4a).
    #[arg(required = true)]
    pub files: Vec<PathBuf>,
    /// Print a JSON array instead of text.
    #[arg(long)]
    pub json: bool,
    /// Include the 512 values in the JSON output.
    #[arg(long, requires = "json")]
    pub full: bool,
    /// Model settings.
    #[command(flatten)]
    pub opts: EmbedOpts,
}

/// `sonar embed-text` arguments.
#[derive(Debug, Clone, Args)]
pub struct EmbedTextArgs {
    /// The query (several words are joined with spaces).
    #[arg(required = true)]
    pub query: Vec<String>,
    /// Print JSON instead of text.
    #[arg(long)]
    pub json: bool,
    /// Include the 512 values in the JSON output.
    #[arg(long, requires = "json")]
    pub full: bool,
    /// Model settings.
    #[command(flatten)]
    pub opts: EmbedOpts,
}

/// `sonar bench embed` arguments.
#[derive(Debug, Clone, Args)]
pub struct BenchEmbedArgs {
    /// Files in the library of the end-to-end run (copies of the fixtures).
    #[arg(long, default_value_t = 1_000, value_parser = clap::value_parser!(u32).range(1..=1_000_000))]
    pub files: u32,
    /// Clips in the model-only run.
    #[arg(long, default_value_t = 2_000, value_parser = clap::value_parser!(u32).range(1..=1_000_000))]
    pub model_clips: u32,
    /// Files per committed batch of the end-to-end run.
    #[arg(long, default_value_t = 128, value_parser = clap::value_parser!(u32).range(1..))]
    pub batch: u32,
    /// Create the working folder inside this directory.
    #[arg(long)]
    pub dir: Option<PathBuf>,
    /// Keep the working folder and print its path.
    #[arg(long)]
    pub keep: bool,
    /// Print JSON instead of text.
    #[arg(long)]
    pub json: bool,
    /// Model settings.
    #[command(flatten)]
    pub opts: EmbedOpts,
}

#[derive(Serialize)]
struct FileReport {
    path: PathBuf,
    ok: bool,
    error: Option<String>,
    dims: Option<usize>,
    norm: Option<f32>,
    partial: bool,
    /// Decode (native rate -> 48 kHz) + front end.
    prep_ms: f64,
    /// Queue + model run.
    model_ms: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    embedding: Option<Vec<f32>>,
}

fn norm(e: &Embedding) -> f32 {
    e.iter().map(|v| v * v).sum::<f32>().sqrt()
}

/// `sonar embed`.
pub fn run(args: &EmbedArgs) -> Result<()> {
    let mut opts = args.opts.to_options();
    if opts.sessions == 0 && opts.ep == EmbedEp::Cpu {
        // Few files: no point loading a session per performance core.
        opts.sessions = args.files.len().clamp(1, embed::default_sessions());
    }
    let pool = EmbedPool::new(&opts).context("loading the CLAP audio model")?;
    let mut prep = AudioPrep::new();
    let mut reports = Vec::new();
    for path in &args.files {
        reports.push(embed_one(&pool, &mut prep, path, args.full));
    }
    if args.json {
        println!("{}", serde_json::to_string_pretty(&reports)?);
    } else {
        for r in &reports {
            match (&r.error, r.dims, r.norm) {
                (None, Some(d), Some(n)) => println!(
                    "{}: {d} dims, norm {n:.4}, decode+front {:.1} ms, model {:.1} ms{}",
                    r.path.display(),
                    r.prep_ms,
                    r.model_ms,
                    if r.partial { " (partial file)" } else { "" }
                ),
                (Some(e), ..) => eprintln!("{}: {e}", r.path.display()),
                _ => {}
            }
        }
    }
    let failed = reports.iter().filter(|r| !r.ok).count();
    if failed > 0 {
        bail!("{failed} of {} files could not be embedded", reports.len());
    }
    Ok(())
}

fn embed_one(pool: &EmbedPool, prep: &mut AudioPrep, path: &Path, full: bool) -> FileReport {
    let mut r = FileReport {
        path: path.to_path_buf(),
        ok: false,
        error: None,
        dims: None,
        norm: None,
        partial: false,
        prep_ms: 0.0,
        model_ms: 0.0,
        embedding: None,
    };
    let t = Instant::now();
    let prepared = match prep.prepare(path, None) {
        Ok(p) => p,
        Err(e) => {
            r.error = Some(e.to_string());
            return r;
        }
    };
    r.prep_ms = t.elapsed().as_secs_f64() * 1000.0;
    r.partial = prepared.partial;
    let t = Instant::now();
    match pool.embed(prepared.features) {
        Ok(e) => {
            r.model_ms = t.elapsed().as_secs_f64() * 1000.0;
            (r.ok, r.dims, r.norm) = (true, Some(EMBED_DIM), Some(norm(&e)));
            r.embedding = full.then(|| e.to_vec());
        }
        Err(e) => r.error = Some(e.to_string()),
    }
    r
}

/// `sonar embed-text`.
pub fn run_text(args: &EmbedTextArgs) -> Result<()> {
    let query = args.query.join(" ");
    let t = Instant::now();
    let mut enc =
        TextEncoder::new(&args.opts.to_options()).context("loading the CLAP text model")?;
    let load_ms = t.elapsed().as_secs_f64() * 1000.0;
    let tokens = enc.token_ids(&query).context("tokenizing the query")?;
    let t = Instant::now();
    let e = enc.embed(&query).context("embedding the query")?;
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    if args.json {
        let mut v = serde_json::json!({
            "query": query, "tokens": tokens, "dims": EMBED_DIM,
            "norm": norm(&e), "load_ms": load_ms, "embed_ms": ms,
        });
        if args.full {
            v["embedding"] = serde_json::json!(e.to_vec());
        }
        println!("{}", serde_json::to_string_pretty(&v)?);
    } else {
        println!(
            "{query:?}: {} tokens, {EMBED_DIM} dims, norm {:.4}, {ms:.1} ms (model load {load_ms:.0} ms)",
            tokens.len(),
            norm(&e)
        );
    }
    Ok(())
}

/// `sonar bench embed`.
pub fn bench(args: &BenchEmbedArgs) -> Result<()> {
    let work = Work::new(args.dir.as_deref(), args.keep)?;
    let opts = EmbedBenchOptions {
        files: args.files as usize,
        model_clips: args.model_clips as usize,
        batch: args.batch as usize,
        embed: args.opts.to_options(),
    };
    let b = bench::bench_embed(work.path(), &opts).context("embed benchmark")?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&b)?);
    } else {
        print_bench(&b);
    }
    Ok(())
}

fn print_bench(b: &EmbedBench) {
    let m = &b.machine;
    println!("machine: {}, {} cores, {} build", m.cpu, m.cores, m.profile);
    println!(
        "ep {}, {} session(s), max batch {}; load {:.2} s",
        b.ep, b.sessions, b.max_batch, b.model.load_secs
    );
    println!(
        "model only : {} clips in {:.2} s = {:.1} clips/s",
        b.model.clips, b.model.secs, b.model.clips_per_sec
    );
    println!(
        "front end  : {:.2} ms/file (decode + log-mel, 1 thread, {} files)",
        b.front_ms_per_file, b.front_files
    );
    if let Some(s) = b.end_to_end.stages.first() {
        println!(
            "end to end : {} files in {:.2} s = {:.1} files/s (ok {}, errors {}, {} batches, commit {:.2} s); budget >= 100 files/s",
            s.done,
            s.seconds,
            s.files_per_sec(),
            s.ok,
            s.errors,
            s.timing.batches,
            s.timing.commit_secs
        );
    }
}
