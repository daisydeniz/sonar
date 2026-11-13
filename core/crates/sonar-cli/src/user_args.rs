//! Argument structs of the user-data commands (T5.5b, ADR-031): `fav`,
//! `hide`, `tag`, `collection`, `override`. Files are given as ids (the
//! `file_id` of `sonar search --json`) or as paths inside a library.

use clap::{Args, Subcommand};

/// Files to act on: ids or paths, mixed freely.
#[derive(Debug, Clone, Args)]
pub struct Targets {
    /// Library file ids or paths of library files.
    #[arg(required = true, value_name = "ID|PATH")]
    pub files: Vec<String>,
}

/// `sonar fav` / `sonar hide`.
#[derive(Debug, Clone, Args)]
pub struct FlagArgs {
    /// Files to mark.
    #[command(flatten)]
    pub targets: Targets,
    /// Remove the mark instead of setting it.
    #[arg(long)]
    pub off: bool,
}

/// `sonar tag` actions.
#[derive(Debug, Subcommand)]
pub enum TagCommand {
    /// Add a tag to files (an existing tag keeps its first spelling).
    Add {
        /// The tag.
        tag: String,
        /// Files to tag.
        #[command(flatten)]
        targets: Targets,
    },
    /// Remove a tag from files.
    Rm {
        /// The tag (case-insensitive).
        tag: String,
        /// Files to untag.
        #[command(flatten)]
        targets: Targets,
    },
    /// List all tags with their file counts.
    List {
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
}

/// `sonar collection` actions.
#[derive(Debug, Subcommand)]
pub enum CollectionCommand {
    /// Create an empty collection and print its id.
    Create {
        /// Name (unique ignoring case).
        name: String,
    },
    /// Rename a collection.
    Rename {
        /// Collection id.
        id: i64,
        /// New name.
        name: String,
    },
    /// Delete a collection (its files stay in the library).
    Delete {
        /// Collection id.
        id: i64,
    },
    /// Append files to a collection.
    Add {
        /// Collection id.
        id: i64,
        /// Files to add.
        #[command(flatten)]
        targets: Targets,
    },
    /// Remove files from a collection.
    Rm {
        /// Collection id.
        id: i64,
        /// Files to remove.
        #[command(flatten)]
        targets: Targets,
    },
    /// List collections, or the files of one.
    List {
        /// Show the files of this collection.
        id: Option<i64>,
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
}

/// `sonar override`.
#[derive(Debug, Clone, Args)]
pub struct OverrideArgs {
    /// Field: `bpm` (plain number), `key` (Am, F#, 8A), `category`
    /// (docs/categories.md) or `kind` (loop, one_shot).
    pub field: String,
    /// Set the override to this value.
    #[arg(
        long,
        value_name = "VALUE",
        conflicts_with = "clear",
        required_unless_present = "clear"
    )]
    pub set: Option<String>,
    /// Remove the override: the detected value applies again.
    #[arg(long)]
    pub clear: bool,
    /// Files to change.
    #[command(flatten)]
    pub targets: Targets,
}
