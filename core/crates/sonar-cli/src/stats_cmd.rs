//! `sonar stats`: library and store statistics.

use anyhow::{Context, Result};
use sonar_core::store::{LibraryStats, StoreStats};
use sonar_core::Store;

/// Prints statistics of the default store as text or JSON.
pub fn run(json: bool) -> Result<()> {
    let store = Store::open_default().context("opening the Sonar store")?;
    let stats = store.stats().context("reading store statistics")?;
    if json {
        println!("{}", serde_json::to_string_pretty(&stats)?);
    } else {
        print_text(&stats);
    }
    Ok(())
}

fn print_text(s: &StoreStats) {
    let path = s
        .path
        .as_deref()
        .map_or_else(|| "(in memory)".into(), |p| p.display().to_string());
    println!(
        "store: {path} (schema {}, db {}, wal {})",
        s.schema_version,
        human_bytes(s.db_bytes),
        human_bytes(s.wal_bytes)
    );
    if s.libraries.is_empty() {
        println!("no libraries yet: run `sonar scan <dir>`");
    }
    for l in &s.libraries {
        print_library(l);
    }
}

fn print_library(l: &LibraryStats) {
    let state = if l.online { "online" } else { "OFFLINE" };
    println!("library {}: {} [{state}]", l.id, l.root);
    let f = &l.files;
    println!(
        "  files: {} (new {}, meta {}, analyzed {}, error {}, missing {})",
        f.total, f.new, f.meta, f.analyzed, f.error, f.missing
    );
    let dur = l.duration_ms.map_or_else(
        || "unknown".to_owned(),
        |ms| format!("{} ({} files)", human_duration(ms), l.files_with_duration),
    );
    println!("  size: {}, duration: {dur}", human_bytes(l.bytes));
    for st in &l.stages {
        println!(
            "  {}: ok {}, partial {}, error {}, pending {}",
            st.stage, st.ok, st.partial, st.error, st.pending
        );
    }
}

fn human_bytes(b: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = b as f64;
    let mut u = 0;
    while v >= 1000.0 && u < UNITS.len() - 1 {
        v /= 1000.0;
        u += 1;
    }
    if u == 0 {
        format!("{b} B")
    } else {
        format!("{v:.1} {}", UNITS[u])
    }
}

fn human_duration(ms: u64) -> String {
    let s = ms / 1000;
    format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_sizes_and_durations() {
        assert_eq!(human_bytes(999), "999 B");
        assert_eq!(human_bytes(1_500_000), "1.5 MB");
        assert_eq!(human_duration(3_723_000), "1:02:03");
    }
}
