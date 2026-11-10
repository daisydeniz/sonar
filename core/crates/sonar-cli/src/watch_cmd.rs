//! `sonar watch`: runs the file watcher over the default store until the
//! process is killed.

use std::io::Write;
use std::time::Duration;

use anyhow::{Context, Result};
use sonar_core::paths;
use sonar_core::watch::{self, WatchEvent, WatchOptions};

/// Starts the watcher and blocks forever (Ctrl-C / SIGTERM end the process;
/// the store is WAL, so an abrupt end loses nothing committed).
pub fn run(json: bool, debounce_ms: u64) -> Result<()> {
    let db = paths::default_db_path().context("locating the Sonar store")?;
    let options = WatchOptions {
        debounce: Duration::from_millis(debounce_ms),
        ..WatchOptions::default()
    };
    let handle = watch::start(&db, options, move |event| print_event(event, json))
        .context("starting the watcher")?;
    eprintln!("watching libraries in {} (Ctrl-C to stop)", db.display());
    while handle.is_running() {
        std::thread::park_timeout(Duration::from_secs(1));
    }
    anyhow::bail!("the watcher stopped unexpectedly")
}

fn print_event(event: &WatchEvent, json: bool) {
    let line = if json {
        serde_json::to_string(event)
            .unwrap_or_else(|e| format!("{{\"event\":\"error\",\"message\":\"{e}\"}}"))
    } else {
        text(event)
    };
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{line}");
    let _ = out.flush();
}

fn text(event: &WatchEvent) -> String {
    match event {
        WatchEvent::Scanned {
            library_id,
            summary: s,
        } => format!(
            "scanned library {library_id}: added {}, changed {}, moved {}, removed {}, restored {}, errors {} ({} ms)",
            s.added, s.changed, s.moved, s.removed, s.restored, s.errors, s.duration_ms
        ),
        WatchEvent::Offline { library_id } => format!("offline library {library_id}"),
        WatchEvent::Online { library_id } => format!("online library {library_id}"),
        WatchEvent::Error {
            library_id,
            message,
        } => match library_id {
            Some(id) => format!("error library {id}: {message}"),
            None => format!("error: {message}"),
        },
        _ => "unknown event".to_owned(),
    }
}
