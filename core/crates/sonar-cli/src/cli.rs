//! Command-line definition (clap derive). Subcommands are added task by task
//! (see docs/ROADMAP.md).
//!
//! Note: `scripts/check.sh` treats `sonar eval --help` exiting 0 as "the eval
//! gate exists", so an `eval` subcommand must only be added together with
//! `--fixtures --check` support.

use std::path::PathBuf;

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

    /// Generate synthetic WAV fixtures plus a `fixtures.json` truth manifest.
    ///
    /// Output is deterministic for a given seed. Truth is only in the
    /// manifest; file names are neutral. See docs/EVAL.md.
    Fixtures {
        /// Output directory (created if missing; existing fixtures are overwritten).
        dir: PathBuf,
        /// PRNG seed for noise content (0..=2^53-1, exact in any JSON reader).
        #[arg(
            long,
            default_value_t = sonar_core::fixtures::DEFAULT_SEED,
            value_parser = clap::value_parser!(u64).range(..=sonar_core::fixtures::MAX_SEED)
        )]
        seed: u64,
        /// Skip the > 5 min file (faster; for quick local runs).
        #[arg(long)]
        skip_long: bool,
    },

    /// Scan a sample library: add DIR as a library if it is new, then bring
    /// the store in line with the disk (added/changed/moved/removed files).
    /// Without DIR, rescan every registered library.
    Scan {
        /// Library root directory.
        dir: Option<PathBuf>,
    },

    /// Decode one audio file the way analysis does (mono, resampled, first
    /// 30 s) and print its source format, true duration and the analysis
    /// sample count. Exits non-zero if the file cannot be decoded.
    Decode {
        /// Audio file (wav, aif/aiff, flac, mp3, ogg, m4a).
        file: PathBuf,
        /// Analysis sample rate in Hz.
        #[arg(
            long,
            default_value_t = sonar_core::decode::ANALYSIS_RATE,
            value_parser = clap::value_parser!(u32).range(
                i64::from(sonar_core::decode::source::MIN_SAMPLE_RATE)
                    ..=i64::from(sonar_core::decode::source::MAX_SAMPLE_RATE)
            )
        )]
        rate: u32,
        /// Analysis window in seconds (0 = duration only).
        #[arg(long, default_value_t = sonar_core::decode::DEFAULT_MAX_ANALYSIS_SECS,
              value_parser = parse_secs, conflicts_with = "full")]
        max_secs: f64,
        /// Decode the whole file (up to the hard memory cap) instead of the window.
        #[arg(long)]
        full: bool,
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },

    /// Inspect the SQLite store (location: $SONAR_HOME/sonar.db, else
    /// ~/Library/Application Support/Sonar/sonar.db).
    Store {
        /// Store action.
        #[command(subcommand)]
        command: StoreCommand,
    },
}

/// `sonar store` subcommands.
#[derive(Debug, Subcommand)]
pub enum StoreCommand {
    /// Open (create + migrate if needed) the store and print its path,
    /// schema version, journal mode and per-table row counts.
    Info,
}

/// Parses a finite, non-negative number of seconds.
fn parse_secs(s: &str) -> Result<f64, String> {
    match s.parse::<f64>() {
        Ok(v) if v.is_finite() && v >= 0.0 => Ok(v),
        _ => Err(format!("'{s}' is not a non-negative number of seconds")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn parses_decode_defaults_and_flags() {
        let cli = Cli::try_parse_from(["sonar", "decode", "a.wav"]).map_err(|e| e.to_string());
        assert!(matches!(
            cli,
            Ok(Cli {
                command: Command::Decode {
                    rate: 22_050,
                    full: false,
                    json: false,
                    max_secs,
                    ..
                }
            }) if max_secs == 30.0
        ));
        let cli = Cli::try_parse_from([
            "sonar",
            "decode",
            "a.wav",
            "--rate",
            "16000",
            "--max-secs",
            "2.5",
            "--json",
        ])
        .map_err(|e| e.to_string());
        assert!(matches!(
            cli,
            Ok(Cli {
                command: Command::Decode {
                    rate: 16_000,
                    json: true,
                    max_secs,
                    ..
                }
            }) if max_secs == 2.5
        ));
        for bad in [
            &["sonar", "decode", "a.wav", "--full", "--max-secs", "3"][..],
            &["sonar", "decode", "a.wav", "--max-secs", "-1"],
            &["sonar", "decode", "a.wav", "--max-secs", "nan"],
            &["sonar", "decode", "a.wav", "--rate", "0"],
            &["sonar", "decode"],
        ] {
            assert!(Cli::try_parse_from(bad).is_err(), "{bad:?}");
        }
        assert!(Cli::try_parse_from(["sonar", "decode", "a.wav", "--full"]).is_ok());
    }

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

    #[test]
    fn parses_store_info_and_requires_an_action() {
        let cli = Cli::try_parse_from(["sonar", "store", "info"]).map_err(|e| e.to_string());
        assert!(matches!(
            cli,
            Ok(Cli {
                command: Command::Store {
                    command: StoreCommand::Info
                }
            })
        ));
        assert!(Cli::try_parse_from(["sonar", "store"]).is_err());
    }

    #[test]
    fn parses_scan_with_and_without_dir() {
        let cli = Cli::try_parse_from(["sonar", "scan", "/x"]).map_err(|e| e.to_string());
        assert!(matches!(
            cli,
            Ok(Cli {
                command: Command::Scan { dir: Some(ref d) }
            }) if d.as_os_str() == "/x"
        ));
        let cli = Cli::try_parse_from(["sonar", "scan"]).map_err(|e| e.to_string());
        assert!(matches!(
            cli,
            Ok(Cli {
                command: Command::Scan { dir: None }
            })
        ));
    }

    #[test]
    fn parses_fixtures_with_defaults_and_flags() {
        let cli = Cli::try_parse_from(["sonar", "fixtures", "/tmp/fx"]).map_err(|e| e.to_string());
        assert!(matches!(
            cli,
            Ok(Cli {
                command: Command::Fixtures {
                    seed: sonar_core::fixtures::DEFAULT_SEED,
                    skip_long: false,
                    ..
                }
            })
        ));
        let cli = Cli::try_parse_from(["sonar", "fixtures", "x", "--seed", "9", "--skip-long"])
            .map_err(|e| e.to_string());
        assert!(matches!(
            cli,
            Ok(Cli {
                command: Command::Fixtures {
                    seed: 9,
                    skip_long: true,
                    ..
                }
            })
        ));
        assert!(Cli::try_parse_from(["sonar", "fixtures"]).is_err());
        let max = sonar_core::fixtures::MAX_SEED;
        let ok = max.to_string();
        let too_big = (max + 1).to_string();
        assert!(Cli::try_parse_from(["sonar", "fixtures", "x", "--seed", ok.as_str()]).is_ok());
        assert!(
            Cli::try_parse_from(["sonar", "fixtures", "x", "--seed", too_big.as_str()]).is_err()
        );
    }
}
