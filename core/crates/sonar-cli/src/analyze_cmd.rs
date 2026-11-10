//! `sonar analyze`: runs the job pipeline over the default store.

use std::io::IsTerminal;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use sonar_core::jobs::{self, JobEvent, JobOptions, JobSummary, Stage};
use sonar_core::paths;

/// Minimum time between progress lines on stderr.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);

/// Parsed arguments.
pub struct Args {
    pub stages: Vec<Stage>,
    pub threads: usize,
    pub batch: usize,
    pub json: bool,
    pub debug_delay_ms: u64,
}

/// Runs the job, prints throttled progress to stderr and the summary to
/// stdout. File-level errors are data, not failures.
pub fn run(args: Args) -> Result<()> {
    let db = paths::default_db_path().context("locating the Sonar store")?;
    let last = Mutex::new(Instant::now() - PROGRESS_INTERVAL);
    let tty = std::io::stderr().is_terminal();
    let on_event = Arc::new(move |e: &JobEvent| report(e, &last, tty));
    let opts = JobOptions {
        stages: args.stages,
        threads: args.threads,
        batch_size: args.batch,
        on_event: Some(on_event),
        debug_file_delay: Duration::from_millis(args.debug_delay_ms),
        ..JobOptions::default()
    };
    let summary = jobs::start(&db, opts)
        .context("starting the job")?
        .wait()
        .context("running the job")?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&summary)?);
    } else {
        print_summary(&summary);
    }
    Ok(())
}

fn report(event: &JobEvent, last: &Mutex<Instant>, tty: bool) {
    match event {
        JobEvent::StageStarted { stage, total } => eprintln!("{stage}: {total} files pending"),
        JobEvent::Progress {
            stage,
            done,
            total,
            errors,
            skipped,
        } => {
            let Ok(mut at) = last.lock() else { return };
            if at.elapsed() < PROGRESS_INTERVAL && done < total {
                return;
            }
            *at = Instant::now();
            let end = if tty { '\r' } else { '\n' };
            eprint!("{stage}: {done}/{total} (errors {errors}, skipped {skipped}){end}");
        }
        JobEvent::StageFinished(_) if tty => eprintln!(),
        _ => {}
    }
}

fn print_summary(s: &JobSummary) {
    if s.stages.is_empty() {
        println!("nothing to do");
    }
    for st in &s.stages {
        println!(
            "{}: done {} of {}, ok {}, partial {}, errors {}, skipped {}, {:.2} s, {:.1} files/s",
            st.stage,
            st.done,
            st.total,
            st.ok,
            st.partial,
            st.errors,
            st.skipped,
            st.seconds,
            st.files_per_sec()
        );
    }
    if s.cancelled {
        println!("cancelled");
    }
}
