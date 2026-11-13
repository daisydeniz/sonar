//! `sonar dupes`: exact and near duplicate groups (ADR-036) through the
//! engine, with paths and sizes.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Args;
use serde::Serialize;
use sonar_core::dupes::{
    DupeKind, DupesOptions, DupesTimings, DEFAULT_NEAR_THRESHOLD, MAX_NEAR_THRESHOLD,
};
use sonar_core::engine::{Engine, EngineOptions};
use sonar_core::paths;

/// `sonar dupes` arguments.
#[derive(Debug, Clone, Args)]
pub struct DupesArgs {
    /// Only byte-identical files.
    #[arg(long, conflicts_with = "near_only")]
    pub exact_only: bool,
    /// Only the same sound in other files (needs `sonar analyze`).
    #[arg(long)]
    pub near_only: bool,
    /// Near threshold: largest per-aspect distance of two files in a near
    /// group (0..=0.5; higher = looser).
    #[arg(long, default_value_t = DEFAULT_NEAR_THRESHOLD, value_parser = parse_threshold)]
    pub threshold: f32,
    /// Groups printed (largest waste first); 0 = all.
    #[arg(long, default_value_t = 50)]
    pub limit: usize,
    /// Print JSON instead of text.
    #[arg(long)]
    pub json: bool,
}

fn parse_threshold(s: &str) -> Result<f32, String> {
    let t: f32 = s.parse().map_err(|e| format!("{e}"))?;
    if t.is_finite() && (0.0..=MAX_NEAR_THRESHOLD).contains(&t) {
        Ok(t)
    } else {
        Err(format!("must be within 0..={MAX_NEAR_THRESHOLD}"))
    }
}

#[derive(Serialize)]
struct Member {
    file_id: i64,
    size: u64,
    path: Option<PathBuf>,
}

#[derive(Serialize)]
struct Group {
    kind: DupeKind,
    wasted_bytes: u64,
    max_distance: f32,
    mean_distance: f32,
    members: Vec<Member>,
}

#[derive(Serialize)]
struct Report {
    files: usize,
    exact_candidates: usize,
    hashed: usize,
    skipped: usize,
    near_files: usize,
    near_pairs: u64,
    threshold: f32,
    groups_total: usize,
    wasted_bytes_total: u64,
    timings: DupesTimings,
    groups: Vec<Group>,
}

/// Finds the groups in the default store and prints them.
pub fn run(args: &DupesArgs) -> Result<()> {
    let engine = Engine::open_with(
        paths::data_dir().context("locating the Sonar store")?,
        EngineOptions {
            rebuild_index: false,
            ..EngineOptions::default()
        },
    )
    .context("opening the Sonar engine")?;
    let opts = DupesOptions {
        exact: !args.near_only,
        near: !args.exact_only,
        threshold: args.threshold,
        cancel: None,
    };
    let r = engine.duplicates(&opts).context("finding duplicates")?;
    let take = if args.limit == 0 {
        r.groups.len()
    } else {
        args.limit
    };
    let groups = r
        .groups
        .iter()
        .take(take)
        .map(|g| {
            let members = g
                .file_ids
                .iter()
                .zip(&g.sizes)
                .map(|(&file_id, &size)| {
                    Ok(Member {
                        file_id,
                        size,
                        path: engine.file_path(file_id)?,
                    })
                })
                .collect::<sonar_core::Result<Vec<_>>>()?;
            Ok(Group {
                kind: g.kind,
                wasted_bytes: g.wasted_bytes,
                max_distance: g.max_distance,
                mean_distance: g.mean_distance,
                members,
            })
        })
        .collect::<sonar_core::Result<Vec<_>>>()
        .context("looking up paths")?;
    let report = Report {
        files: r.files,
        exact_candidates: r.exact_candidates,
        hashed: r.hashed,
        skipped: r.skipped,
        near_files: r.near_files,
        near_pairs: r.near_pairs,
        threshold: r.threshold,
        groups_total: r.groups.len(),
        wasted_bytes_total: r.groups.iter().map(|g| g.wasted_bytes).sum(),
        timings: r.timings,
        groups,
    };
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_text(&report);
    }
    Ok(())
}

fn print_text(r: &Report) {
    for (n, g) in r.groups.iter().enumerate() {
        let kind = match g.kind {
            DupeKind::Exact => "exact".to_owned(),
            DupeKind::Near => format!("near, max {:.3}", g.max_distance),
        };
        println!(
            "#{} ({kind}, {} files, {:.1} MB wasted)",
            n + 1,
            g.members.len(),
            g.wasted_bytes as f64 / 1e6
        );
        for m in &g.members {
            let path = m
                .path
                .as_ref()
                .map_or_else(|| "(gone)".into(), |p| p.display().to_string());
            println!("  #{:<7} {:>10} B  {path}", m.file_id, m.size);
        }
    }
    println!(
        "{} groups, {:.1} MB wasted; {} files, {} exact candidates ({} hashed, {} skipped), \
         {} near files, {} pairs in the duration window, threshold {}",
        r.groups_total,
        r.wasted_bytes_total as f64 / 1e6,
        r.files,
        r.exact_candidates,
        r.hashed,
        r.skipped,
        r.near_files,
        r.near_pairs,
        r.threshold
    );
    println!(
        "load {:.1} ms, exact {:.1} ms, near {:.1} ms",
        r.timings.load_ms, r.timings.exact_ms, r.timings.near_ms
    );
}

#[cfg(test)]
mod tests {
    use crate::cli::{Cli, Command};
    use clap::Parser;

    #[test]
    fn parses_modes_and_threshold() {
        let cli = Cli::try_parse_from(["sonar", "dupes", "--near-only", "--threshold", "0.2"]);
        match cli.map(|c| c.command) {
            Ok(Command::Dupes(a)) => {
                assert!(a.near_only && !a.exact_only);
                assert_eq!(a.threshold, 0.2);
                assert_eq!(a.limit, 50);
            }
            other => panic!("{other:?}"),
        }
        for bad in [
            &["sonar", "dupes", "--exact-only", "--near-only"][..],
            &["sonar", "dupes", "--threshold", "0.9"],
            &["sonar", "dupes", "--threshold", "-1"],
            &["sonar", "dupes", "--threshold", "nan"],
        ] {
            assert!(Cli::try_parse_from(bad).is_err(), "{bad:?}");
        }
    }
}
