//! `sonar fav|hide|tag|collection|override`: user data through the
//! [`Engine`], which refreshes the search index for the touched files before
//! it returns (ADR-031).

use std::path::Path;

use anyhow::{bail, Context, Result};
use sonar_core::engine::{Engine, EngineOptions};
use sonar_core::paths;
use sonar_core::store::OverrideField;

use crate::user_args::{CollectionCommand, FlagArgs, OverrideArgs, TagCommand, Targets};

/// Opens the engine; an index that does not exist yet is built by the first
/// search, not here.
pub(crate) fn open() -> Result<Engine> {
    Engine::open_with(
        paths::data_dir().context("locating the Sonar store")?,
        EngineOptions {
            rebuild_index: false,
            ..EngineOptions::default()
        },
    )
    .context("opening the Sonar engine")
}

/// Resolves ids and paths to file ids; an unknown path is an error.
fn resolve(engine: &Engine, targets: &Targets) -> Result<Vec<i64>> {
    targets
        .files
        .iter()
        .map(|t| resolve_one(engine, t))
        .collect()
}

/// One id or path of a library file to its file id.
pub(crate) fn resolve_one(engine: &Engine, t: &str) -> Result<i64> {
    match t.parse::<i64>() {
        Ok(id) => Ok(id),
        Err(_) => {
            let path =
                std::path::absolute(Path::new(t)).with_context(|| format!("resolving {t}"))?;
            engine
                .file_id_by_path(&path)
                .context("looking up the file")?
                .with_context(|| format!("not a file of any library: {t}"))
        }
    }
}

fn files(n: usize) -> String {
    format!("{n} file{}", if n == 1 { "" } else { "s" })
}

/// `sonar fav` (`hide` with `hidden`).
pub fn flag(args: &FlagArgs, hidden: bool) -> Result<()> {
    let engine = open()?;
    let ids = resolve(&engine, &args.targets)?;
    let on = !args.off;
    let changed = if hidden {
        engine.set_hidden(&ids, on)
    } else {
        engine.set_favorite(&ids, on)
    }
    .context("saving")?;
    let what = if hidden { "hidden" } else { "favourite" };
    let state = if on { "marked" } else { "unmarked" };
    println!(
        "{state} as {what}: {} changed, {} given",
        files(changed),
        ids.len()
    );
    Ok(())
}

/// `sonar tag ...`.
pub fn tag(command: &TagCommand) -> Result<()> {
    let engine = open()?;
    match command {
        TagCommand::Add { tag, targets } => {
            let ids = resolve(&engine, targets)?;
            let n = engine
                .add_tags(&ids, std::slice::from_ref(tag))
                .context("tagging")?;
            println!("tag '{}' added to {}", tag.trim(), files(n));
        }
        TagCommand::Rm { tag, targets } => {
            let ids = resolve(&engine, targets)?;
            let n = engine
                .remove_tags(&ids, std::slice::from_ref(tag))
                .context("untagging")?;
            println!("tag '{}' removed from {}", tag.trim(), files(n));
        }
        TagCommand::List { json } => {
            let tags = engine.tags().context("listing tags")?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&tags)?);
            } else {
                for t in &tags {
                    println!("{:>6}  {}", t.files, t.name);
                }
            }
        }
    }
    Ok(())
}

/// `sonar collection ...`.
pub fn collection(command: &CollectionCommand) -> Result<()> {
    let engine = open()?;
    match command {
        CollectionCommand::Create { name } => {
            let id = engine.create_collection(name).context("creating")?;
            println!("collection {id}: {}", name.trim());
        }
        CollectionCommand::Rename { id, name } => {
            engine.rename_collection(*id, name).context("renaming")?;
            println!("collection {id} renamed to {}", name.trim());
        }
        CollectionCommand::Delete { id } => {
            if !engine.delete_collection(*id).context("deleting")? {
                bail!("collection {id} not found");
            }
            println!("collection {id} deleted");
        }
        CollectionCommand::Add { id, targets } => {
            let ids = resolve(&engine, targets)?;
            let n = engine.add_to_collection(*id, &ids).context("adding")?;
            println!("{} added to collection {id}", files(n));
        }
        CollectionCommand::Rm { id, targets } => {
            let ids = resolve(&engine, targets)?;
            let n = engine
                .remove_from_collection(*id, &ids)
                .context("removing")?;
            println!("{} removed from collection {id}", files(n));
        }
        CollectionCommand::List { id: None, json } => {
            let all = engine.collections().context("listing")?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&all)?);
            } else {
                for c in &all {
                    println!("{:>4}  {:>6} files  {}", c.id, c.files, c.name);
                }
            }
        }
        CollectionCommand::List { id: Some(id), json } => {
            let items = engine.collection_items(*id).context("listing")?;
            let rows: Vec<_> = items
                .iter()
                .map(|&f| Ok((f, engine.file_path(f)?)))
                .collect::<sonar_core::Result<_>>()
                .context("looking up paths")?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&rows)?);
            } else {
                for (f, p) in &rows {
                    let p: &Option<std::path::PathBuf> = p;
                    println!(
                        "#{f:<7} {}",
                        p.as_deref()
                            .map_or_else(String::new, |p| p.display().to_string())
                    );
                }
            }
        }
    }
    Ok(())
}

/// `sonar override`.
pub fn override_field(args: &OverrideArgs) -> Result<()> {
    let engine = open()?;
    let field = OverrideField::parse(&args.field)?;
    let ids = resolve(&engine, &args.targets)?;
    match (&args.set, args.clear) {
        (Some(value), false) => {
            let n = engine.set_override(&ids, field, value).context("saving")?;
            println!("{} = {} on {}", field.as_str(), value.trim(), files(n));
        }
        _ => {
            let n = engine.clear_override(&ids, field).context("saving")?;
            println!("{} override removed from {}", field.as_str(), files(n));
        }
    }
    Ok(())
}
