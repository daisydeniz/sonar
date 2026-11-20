//! `sonar tags list|rename|merge|delete|similar|ignore` (T12.2, ADR-043):
//! library-wide tag management through the [`Engine`](sonar_core::engine::Engine).
//! Every change prints the ids of the files it touched (`--json` for the
//! machine-readable form).

use anyhow::{Context, Result};
use clap::Subcommand;

use crate::user_cmd::open;

/// `sonar tags` actions.
#[derive(Debug, Subcommand)]
pub enum TagsCommand {
    /// List all tags with their file counts (unused ones included).
    List {
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Rename a tag; onto an existing tag it merges (that spelling stays).
    Rename {
        /// Current name (case-insensitive).
        from: String,
        /// New name.
        to: String,
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Merge several tags into one (created if new).
    Merge {
        /// The tag that remains.
        #[arg(long)]
        into: String,
        /// Tags to merge.
        #[arg(required = true)]
        sources: Vec<String>,
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Delete a tag from every file.
    Delete {
        /// The tag.
        name: String,
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Suggest tags that look like the same word (not the ignored pairs).
    Similar {
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Never suggest this pair as similar again.
    Ignore {
        /// First tag.
        a: String,
        /// Second tag.
        b: String,
    },
}

fn report(what: &str, ids: &[i64], json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::json!({ "files": ids }));
    } else {
        println!("{what}: {} file(s) affected {ids:?}", ids.len());
    }
    Ok(())
}

/// Runs one `sonar tags` action.
pub fn run(command: &TagsCommand) -> Result<()> {
    let engine = open()?;
    match command {
        TagsCommand::List { json } => {
            let tags = engine.tags().context("listing tags")?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&tags)?);
            } else {
                for t in &tags {
                    println!("{:>6}  {}", t.files, t.name);
                }
            }
        }
        TagsCommand::Rename { from, to, json } => {
            let ids = engine.rename_tag(from, to).context("renaming")?;
            report("renamed", &ids, *json)?;
        }
        TagsCommand::Merge {
            into,
            sources,
            json,
        } => {
            let ids = engine.merge_tags(sources, into).context("merging")?;
            report("merged", &ids, *json)?;
        }
        TagsCommand::Delete { name, json } => {
            let ids = engine.delete_tag(name).context("deleting")?;
            report("deleted", &ids, *json)?;
        }
        TagsCommand::Similar { json } => {
            let pairs = engine.similar_tags().context("finding similar tags")?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&pairs)?);
            } else {
                for p in &pairs {
                    println!(
                        "{:?}: {} ({}) ~ {} ({})",
                        p.reason, p.a, p.a_files, p.b, p.b_files
                    );
                }
            }
        }
        TagsCommand::Ignore { a, b } => {
            engine.ignore_similar_tags(a, b).context("ignoring pair")?;
            println!("ignored: {a} / {b}");
        }
    }
    Ok(())
}
