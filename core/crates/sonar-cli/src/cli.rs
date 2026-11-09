//! Command-line definition (clap derive). Subcommands are added task by task
//! (see docs/ROADMAP.md).
//!
//! Note: `scripts/check.sh` treats `sonar eval --help` exiting 0 as "the eval
//! gate exists", so an `eval` subcommand must only be added together with
//! `--fixtures --check` support.

use clap::{Parser, Subcommand};

/// `sonar` — headless access to every Sonar core feature.
#[derive(Debug, Parser)]
#[command(
    name = "sonar",
    version = sonar_core::version(),
    about = "Headless access to every Sonar core feature.",
    after_help = "Logging: set RUST_LOG (e.g. RUST_LOG=debug); default is warn, written to stderr.",
    subcommand_required = true,
    arg_required_else_help = true
)]
pub struct Cli {
    /// What to run.
    #[command(subcommand)]
    pub command: Command,
}

/// Top-level subcommands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Print the core library version.
    Version,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn definition_is_consistent() {
        Cli::command().debug_assert();
    }

    #[test]
    fn parses_version_subcommand() {
        let cli = Cli::try_parse_from(["sonar", "version"]).map_err(|e| e.to_string());
        assert!(matches!(
            cli,
            Ok(Cli {
                command: Command::Version
            })
        ));
    }
}
