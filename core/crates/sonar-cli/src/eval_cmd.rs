//! `sonar eval`: measure the analysis against fixtures, silver and golden
//! data (see `docs/EVAL.md`). All logic lives in `sonar_core::eval`.

use std::path::{Path, PathBuf};
use std::process::Command as Proc;

use anyhow::{bail, Context, Result};
use clap::Args;
use sonar_core::classify::category::clap::ClapScorer;
use sonar_core::embed::EmbedOptions;
use sonar_core::eval::golden::TEMPLATE_SIZE;
use sonar_core::eval::{self, Baseline, Dataset, EvalReport};

/// Environment variable naming the golden directory.
const GOLDEN_ENV: &str = "SONAR_GOLDEN_DIR";

/// Arguments of `sonar eval`.
#[derive(Debug, Args)]
pub struct EvalArgs {
    /// Evaluate the synthetic fixtures (generated into a temp dir).
    #[arg(long)]
    pub fixtures: bool,
    /// Leave out the 310 s fixture (saves ~0.5 s; only its first 30 s are
    /// analysed, but it is part of the accepted baseline numbers).
    #[arg(long, requires = "fixtures")]
    pub skip_long: bool,
    /// Evaluate real files whose names carry labels (`..._124bpm_Fmin_...`,
    /// `Kick_OneShot`). Names are hidden from the analyzer.
    #[arg(long, value_name = "DIR")]
    pub silver: Option<PathBuf>,
    /// Evaluate the hand-labeled set in `$SONAR_GOLDEN_DIR` (`labels.csv`).
    #[arg(long)]
    pub golden: bool,
    /// Golden directory (default: `$SONAR_GOLDEN_DIR`).
    #[arg(long, value_name = "DIR")]
    pub golden_dir: Option<PathBuf>,
    /// Write a `labels.csv` template with a stratified sample of ~300 audio
    /// files from DIR into the golden directory, for hand labeling. Never
    /// overwrites an existing `labels.csv`.
    #[arg(long, value_name = "DIR", conflicts_with_all = ["fixtures", "silver", "golden", "check", "write_baseline"])]
    pub init_golden: Option<PathBuf>,
    /// Also score a fixed grid of aspect weights and L1/L2 distances for
    /// similarity (report only, never recorded in the baseline).
    #[arg(long, conflicts_with_all = ["check", "write_baseline"])]
    pub similar_sweep: bool,
    /// Score categories without CLAP even when the model is available
    /// (DSP only, as on a machine without `models/`).
    #[arg(long)]
    pub no_clap: bool,
    /// Write per-file category evidence (JSON lines) to FILE: DSP cues,
    /// decisions and, with CLAP, the cosine to every prompt (for tuning the
    /// classifier, ADR-028).
    #[arg(long, value_name = "FILE", hide = true)]
    pub category_dump: Option<PathBuf>,
    /// Exit non-zero when a tracked metric regressed against the baseline
    /// (a percentage dropped by more than 1 point, or a must-be-zero count
    /// rose). Only tiers present in the baseline are compared.
    #[arg(long, conflicts_with = "write_baseline")]
    pub check: bool,
    /// Record this run's numbers as the accepted baseline (other tiers in
    /// the file are kept).
    #[arg(long)]
    pub write_baseline: bool,
    /// Baseline file (default: `eval/reports/BASELINE.md` of the repo).
    #[arg(long, value_name = "FILE")]
    pub baseline: Option<PathBuf>,
    /// Report directory (default: `eval/reports` of the repo).
    #[arg(long, value_name = "DIR")]
    pub out: Option<PathBuf>,
    /// Print the JSON report instead of the markdown one.
    #[arg(long)]
    pub json: bool,
}

/// Runs the command.
pub fn run(args: &EvalArgs) -> Result<()> {
    if let Some(src) = &args.init_golden {
        return init_golden(src, args.golden_dir.as_deref());
    }
    if !(args.fixtures || args.silver.is_some() || args.golden) {
        bail!("choose what to evaluate: --fixtures, --silver <DIR>, --golden (or --init-golden <DIR>)");
    }
    let root = repo_root();
    let out_dir = args
        .out
        .clone()
        .unwrap_or_else(|| root.join("eval/reports"));
    let baseline_path = args
        .baseline
        .clone()
        .unwrap_or_else(|| root.join("eval/reports/BASELINE.md"));

    let opts = eval::EvalOptions {
        similar_sweep: args.similar_sweep,
        clap: if args.no_clap { None } else { load_clap() },
        category_dump: args.category_dump.clone(),
    };
    let mut tiers = Vec::new();
    // Keeps the generated fixtures alive until the end of the run.
    let mut _fixture_dir = None;
    if args.fixtures {
        let tmp = tempfile::tempdir().context("creating a temp dir for the fixtures")?;
        eprintln!("generating fixtures...");
        let ds = eval::fixtures_dataset(tmp.path(), !args.skip_long)
            .context("generating the fixtures")?;
        tiers.push(analyse(&ds, opts.clone()));
        _fixture_dir = Some(tmp);
    }
    if let Some(dir) = &args.silver {
        let ds = eval::silver_dataset(dir)
            .with_context(|| format!("reading the silver set {}", dir.display()))?;
        if ds.items.is_empty() {
            bail!(
                "silver set {}: no audio file with a label in its name ({} files without)",
                dir.display(),
                ds.unlabeled
            );
        }
        tiers.push(analyse(&ds, opts.clone()));
    }
    if args.golden {
        let dir = golden_dir(args.golden_dir.as_deref())?;
        let ds = eval::golden_dataset(&dir)
            .with_context(|| format!("reading the golden set {}", dir.display()))?;
        if ds.items.is_empty() {
            bail!(
                "golden set {}: no labeled file found ({} rows without labels, {} missing files)",
                dir.display(),
                ds.unlabeled,
                ds.missing.len()
            );
        }
        tiers.push(analyse(&ds, opts));
    }

    let report = EvalReport::new(&git_sha(&root), tiers);
    let old = read_baseline(&baseline_path);
    let (json, md) = report
        .write(&out_dir, old.as_ref().ok())
        .with_context(|| format!("writing the report to {}", out_dir.display()))?;
    if args.json {
        println!("{}", report.to_json());
    } else {
        print!("{}", report.to_markdown(old.as_ref().ok()));
    }
    eprintln!("report: {}\n        {}", md.display(), json.display());

    if args.write_baseline {
        // A missing or placeholder baseline starts empty; a broken one is
        // not silently replaced.
        let mut b = match old {
            Ok(b) => b,
            Err(_) if !baseline_path.exists() || is_placeholder(&baseline_path) => {
                Baseline::default()
            }
            Err(e) => bail!("{}: {e}; fix or delete it first", baseline_path.display()),
        };
        b.update(&report);
        std::fs::write(&baseline_path, b.render())
            .with_context(|| format!("writing {}", baseline_path.display()))?;
        eprintln!("baseline written: {}", baseline_path.display());
    } else if args.check {
        check(&report, old, &baseline_path)?;
    }
    Ok(())
}

/// CLAP for the category scores when the model is available (a note on
/// stderr either way); the tracked fixture metrics never depend on it.
pub(crate) fn load_clap() -> Option<std::sync::Arc<ClapScorer>> {
    let opts = EmbedOptions::default();
    if let Some(reason) = opts.unavailable_reason() {
        eprintln!("categories: DSP only ({reason})");
        return None;
    }
    match ClapScorer::load(&opts) {
        Ok(c) => {
            eprintln!("categories: CLAP + DSP");
            Some(std::sync::Arc::new(c))
        }
        Err(e) => {
            eprintln!("categories: DSP only (CLAP failed to load: {e})");
            None
        }
    }
}

/// Evaluates one dataset with a progress line on stderr.
fn analyse(ds: &Dataset, opts: eval::EvalOptions) -> eval::TierReport {
    eprintln!(
        "evaluating {}: {} files...",
        ds.tier.as_str(),
        ds.items.len() + ds.similar_extra.len()
    );
    eval::evaluate_with(ds, opts)
}

/// `--check`: fail on regressions, or when there is nothing to compare to.
fn check(
    report: &EvalReport,
    baseline: Result<Baseline, eval::baseline::BaselineError>,
    path: &Path,
) -> Result<()> {
    let baseline = baseline.map_err(|e| {
        anyhow::anyhow!(
            "cannot check against {}: {e}\nhint: run `sonar eval --fixtures --write-baseline` to record one",
            path.display()
        )
    })?;
    let c = eval::compare(&baseline, report);
    for t in &c.no_baseline {
        eprintln!("note: tier {t} has no baseline, not checked");
    }
    if c.compared.is_empty() {
        bail!(
            "no tier of this run is in {}\nhint: run with --write-baseline first",
            path.display()
        );
    }
    if !c.regressions.is_empty() {
        let list: Vec<String> = c.regressions.iter().map(|r| format!("  {r}")).collect();
        bail!(
            "eval regression vs {} (more than 1 point, or a must-be-zero count rose):\n{}",
            path.display(),
            list.join("\n")
        );
    }
    eprintln!("eval check passed ({})", c.compared.join(", "));
    Ok(())
}

/// `--init-golden`: write the labels template.
fn init_golden(src: &Path, dir: Option<&Path>) -> Result<()> {
    let dir = golden_dir_for_init(dir)?;
    let (path, n) = eval::init_template(src, &dir, TEMPLATE_SIZE)
        .with_context(|| format!("writing a golden template for {}", src.display()))?;
    println!(
        "wrote {n} rows to {}\nFill category, kind (loop|oneshot), bpm, key (e.g. F minor), note (e.g. A4) \
         where you are sure; leave the rest empty. Then run `sonar eval --golden`.",
        path.display()
    );
    Ok(())
}

/// Golden directory for `--init-golden`: the flag, else `$SONAR_GOLDEN_DIR`
/// (created if needed).
fn golden_dir_for_init(flag: Option<&Path>) -> Result<PathBuf> {
    match flag.map(Path::to_path_buf).or_else(env_golden) {
        Some(d) => Ok(d),
        None => bail!("set {GOLDEN_ENV} (or pass --golden-dir) to say where labels.csv goes"),
    }
}

/// Golden directory for `--golden`: must exist.
fn golden_dir(flag: Option<&Path>) -> Result<PathBuf> {
    let Some(dir) = flag.map(Path::to_path_buf).or_else(env_golden) else {
        bail!("--golden needs ${GOLDEN_ENV} (or --golden-dir) pointing at a directory with labels.csv; see `sonar eval --init-golden`");
    };
    if !dir.is_dir() {
        bail!("golden directory {} does not exist", dir.display());
    }
    Ok(dir)
}

fn env_golden() -> Option<PathBuf> {
    std::env::var_os(GOLDEN_ENV)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// Baseline file contents; `Err` when absent or without a JSON block.
fn read_baseline(path: &Path) -> Result<Baseline, eval::baseline::BaselineError> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| eval::baseline::BaselineError(format!("{}: {e}", path.display())))?;
    Baseline::parse(&text)
}

/// The file exists but holds no baseline yet (the committed placeholder).
fn is_placeholder(path: &Path) -> bool {
    std::fs::read_to_string(path).is_ok_and(|t| !t.contains("```json"))
}

/// Repository root (`git rev-parse --show-toplevel`), else the current dir.
fn repo_root() -> PathBuf {
    git(&["rev-parse", "--show-toplevel"], Path::new("."))
        .map_or_else(|| PathBuf::from("."), PathBuf::from)
}

/// Short revision of HEAD, `nogit` outside a repository.
fn git_sha(root: &Path) -> String {
    git(&["rev-parse", "--short", "HEAD"], root).unwrap_or_else(|| "nogit".into())
}

fn git(args: &[&str], dir: &Path) -> Option<String> {
    let out = Proc::new("git").args(args).current_dir(dir).output().ok()?;
    let s = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (out.status.success() && !s.is_empty()).then_some(s)
}
