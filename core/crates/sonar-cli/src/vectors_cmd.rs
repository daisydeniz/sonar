//! `sonar vectors`: aspect vector store statistics and compaction.

use anyhow::{Context, Result};
use sonar_core::index::vectors::{self, VectorStats, VectorStore};

/// Prints the default vector store's stats (after compacting it with
/// `compact`) as text or JSON.
pub fn run(compact: bool, json: bool) -> Result<()> {
    let dir = vectors::default_dir().context("locating the vector store")?;
    let stats = if compact {
        // Refuse to create an empty store on a fresh install.
        VectorStore::open_read(&dir)
            .context("opening the vector store (run `sonar analyze` first)")?;
        let (mut vs, _) = VectorStore::open(&dir).context("opening the vector store")?;
        let r = vs.compact().context("compacting the vector store")?;
        if !json {
            println!(
                "compacted: {} → {} rows in {:.2} s",
                r.rows_before, r.rows_after, r.seconds
            );
        }
        vs.stats()
    } else {
        VectorStore::open_read(&dir)
            .context("opening the vector store (run `sonar analyze` first)")?
            .stats()
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&stats)?);
    } else {
        print_text(&stats);
    }
    Ok(())
}

fn print_text(s: &VectorStats) {
    println!(
        "vector store: {} (generation {})",
        s.dir.display(),
        s.generation
    );
    println!(
        "  rows: {} (live {}, dead {}), stats over {} rows, {} bytes",
        s.rows, s.live, s.dead, s.norm_count, s.bytes
    );
    let dims: Vec<String> = s.aspects.iter().map(|(a, d)| format!("{a} {d}")).collect();
    println!("  raw dims: {}", dims.join(", "));
}
