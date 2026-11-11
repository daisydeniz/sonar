//! Command-line definition (clap derive). Subcommands are added task by task
//! (see docs/ROADMAP.md).
//!
//! Note: `scripts/check.sh` treats `sonar eval --help` exiting 0 as "the eval
//! gate exists", so an `eval` subcommand must only be added together with
//! `--fixtures --check` support.

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use sonar_core::jobs::Stage;

use crate::search_args::FilterArgs;

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

    /// Print basic metadata of audio files: duration, sample rate, channels,
    /// bit depth, sample peak (dBFS) and BS.1770 integrated loudness (LUFS).
    /// Unreadable files are reported and skipped; exits non-zero if any failed.
    Meta {
        /// Audio files (wav, aif/aiff, flac, mp3, ogg, m4a).
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Measure peak/loudness over at most this many seconds from the start.
        #[arg(long, default_value_t = sonar_core::meta::DEFAULT_MAX_META_SECS,
              value_parser = parse_secs)]
        max_meta_secs: f64,
        /// Print a JSON array instead of text.
        #[arg(long)]
        json: bool,
    },

    /// Decode audio files like analysis does (mono 22.05 kHz, first 30 s)
    /// and print a DSP feature summary: frame count, MFCC means and onset
    /// envelope stats. Unreadable files are reported and skipped; exits
    /// non-zero if any failed.
    Dsp {
        /// Audio files (wav, aif/aiff, flac, mp3, ogg, m4a).
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Print a JSON array instead of text.
        #[arg(long)]
        json: bool,
    },

    /// Compute the similarity aspect vectors (Spectrum, Timbre, Pitch,
    /// Amplitude) of audio files from their analysis window and print a
    /// summary. With two or more files, also normalizes them with stats
    /// over the given files and prints distances to the first file
    /// (including Overall). Unreadable files are reported and skipped;
    /// exits non-zero if any failed.
    Aspects {
        /// Audio files (wav, aif/aiff, flac, mp3, ogg, m4a).
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Print JSON (full raw and normalized vectors) instead of text.
        #[arg(long)]
        json: bool,
    },

    /// Classify audio files as loop or one-shot from their analysis window
    /// (onset periodicity, true duration, decay tail) plus a lower-weight
    /// hint from the file and folder names, and print the decision,
    /// confidence and heuristic scores. Unreadable files are reported and
    /// skipped; exits non-zero if any failed.
    Classify {
        /// Audio files (wav, aif/aiff, flac, mp3, ogg, m4a).
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Print a JSON array instead of text.
        #[arg(long)]
        json: bool,
        /// Ignore file and folder names (audio evidence only, as the eval).
        #[arg(long)]
        no_names: bool,
    },

    /// Estimate the tempo of audio files: loop/one-shot decision first
    /// (one-shots never get a BPM), then onset autocorrelation with octave
    /// correction, the loop-length prior and a lower-weight BPM from the
    /// file and folder names. Prints kind, BPM, confidence and source.
    /// Unreadable files are reported and skipped; exits non-zero if any
    /// failed.
    Bpm {
        /// Audio files (wav, aif/aiff, flac, mp3, ogg, m4a).
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Print a JSON array instead of text.
        #[arg(long)]
        json: bool,
        /// Ignore file and folder names (audio evidence only, as the eval).
        #[arg(long)]
        no_names: bool,
    },

    /// Estimate the musical key (tonal chroma vs. key profiles, with a
    /// lower-weight key from the names) and, for pitched one-shots, the
    /// note (YIN). Drums, noise and silence get neither. Unreadable files
    /// are reported and skipped; exits non-zero if any failed.
    Key(crate::key_cmd::KeyArgs),

    /// Measure loop/one-shot, BPM, key and note accuracy against fixtures,
    /// a silver set (labels in file names) and/or the golden set, write a
    /// report to eval/reports and optionally check it against BASELINE.md.
    Eval(crate::eval_cmd::EvalArgs),

    /// Run the analysis pipeline over every library in the store: `meta`
    /// (format, duration) for all files first, then `dsp` (analysis decode,
    /// peak, loudness). Resumable: only pending files are processed, so it can
    /// be interrupted and restarted. Progress goes to stderr, the summary
    /// to stdout. File-level errors are recorded as data (exit code 0).
    Analyze {
        /// Stages to run, comma separated (`meta`, `dsp`; `embed` is not
        /// implemented yet and is skipped).
        #[arg(long, value_delimiter = ',', default_values = ["meta", "dsp"])]
        stages: Vec<Stage>,
        /// Worker threads (0 = all cores).
        #[arg(long, default_value_t = 0)]
        threads: usize,
        /// Files per committed batch (one database transaction each).
        #[arg(long, default_value_t = 128, value_parser = clap::value_parser!(u32).range(1..))]
        batch: u32,
        /// Print the summary as JSON.
        #[arg(long)]
        json: bool,
        /// Test only: sleep this many ms before every file.
        #[arg(long, hide = true, default_value_t = 0)]
        debug_delay_ms: u64,
    },

    /// Watch every library in the store and keep the index in line with the
    /// disk until killed (Ctrl-C): added, deleted, renamed and moved files
    /// are picked up by a debounced incremental rescan; a library whose
    /// folder disappears (unmounted drive) goes offline with its rows kept,
    /// and comes back when the folder returns. Prints one line (or JSON
    /// object) per event on stdout. Does not analyze: run `sonar analyze`.
    Watch {
        /// Print one JSON object per line instead of text.
        #[arg(long)]
        json: bool,
        /// Quiet period after the last change before rescanning (ms).
        #[arg(long, default_value_t = 400, value_parser = clap::value_parser!(u64).range(1..=60_000))]
        debounce_ms: u64,
    },

    /// Library and store statistics: per library the root, online state,
    /// file counts by status, total size and duration, per-stage progress
    /// (ok/partial/error/pending), and the database size.
    Stats {
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },

    /// Show how file names, paths or queries are tokenized for search
    /// (separators, camelCase, `120bpm`, keys → `key:c#m`/`note:c#`, pack
    /// noise dropped) and the BPM/key priors read from the file name.
    /// Tokens sharing a position are joined by `|`.
    Tokenize {
        /// File names, paths or query strings.
        #[arg(required = true)]
        inputs: Vec<String>,
        /// Tokenize as a search query (lowercase keys like `am`, `eb` also
        /// emit `key:`/`note:` tokens) instead of as an indexed name.
        #[arg(long)]
        query: bool,
        /// Print a JSON array instead of text.
        #[arg(long)]
        json: bool,
    },

    /// Rebuild the text search index from the store (file names, folders,
    /// tags, category, kind). `sonar search` builds it on first use and
    /// `sonar scan` refreshes an existing one; this forces a full rebuild.
    Index {
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },

    /// Search the library by text. Every word narrows; `-word` or
    /// `-"two words"` excludes; synonyms (`bd` = kick, `hh` = hihat,
    /// `one shot` = oneshot) from docs/synonyms.tsv or
    /// $SONAR_HOME/synonyms.tsv; keys and tempos are understood (`Cmin`,
    /// `am`, `120 bpm`). The last word also matches as a prefix unless the
    /// query ends with a space (as-you-type). No query lists everything.
    /// Prints rank, score, file id and path. Options go before the query:
    /// from the first query word on, everything is query text. Filters
    /// (`--category kick --bpm 120-128 --key Am --compatible`) work with
    /// or without query words (no words: browse every match by name).
    Search {
        /// Query words (`kick -vinyl`, `"one shot" am`); after the options.
        #[arg(allow_hyphen_values = true)]
        query: Vec<String>,
        /// Maximum number of results printed.
        #[arg(long, default_value_t = 20)]
        limit: usize,
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
        /// Match the last word exactly (no as-you-type prefix matching).
        #[arg(long)]
        no_prefix: bool,
        /// Synonyms file to use instead of the default.
        #[arg(long, value_name = "FILE")]
        synonyms: Option<PathBuf>,
        /// Filters, sort and facets.
        #[command(flatten)]
        filters: FilterArgs,
    },

    /// Performance benchmarks in a temporary folder with its own store
    /// (never yours). Use a release build for meaningful numbers.
    Bench {
        /// Which benchmark.
        #[command(subcommand)]
        command: crate::bench_cmd::BenchCommand,
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
    fn parses_meta_files_and_flags() {
        let cli = Cli::try_parse_from(["sonar", "meta", "a.wav", "b.flac"]);
        match cli.map(|c| c.command) {
            Ok(Command::Meta {
                files,
                max_meta_secs,
                json: false,
            }) => {
                assert_eq!(files.len(), 2);
                assert_eq!(max_meta_secs, sonar_core::meta::DEFAULT_MAX_META_SECS);
            }
            other => panic!("{other:?}"),
        }
        let cli = Cli::try_parse_from(["sonar", "meta", "a.wav", "--json", "--max-meta-secs", "5"]);
        assert!(matches!(
            cli.map(|c| c.command),
            Ok(Command::Meta { json: true, max_meta_secs, .. }) if max_meta_secs == 5.0
        ));
        for bad in [
            &["sonar", "meta"][..],
            &["sonar", "meta", "a.wav", "--max-meta-secs", "-1"],
            &["sonar", "meta", "a.wav", "--max-meta-secs", "nan"],
        ] {
            assert!(Cli::try_parse_from(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn parses_search_words_exclusions_and_flags() {
        let cli = Cli::try_parse_from([
            "sonar", "search", "--limit", "5", "--json", "kick", "-vinyl", "dark",
        ]);
        match cli.map(|c| c.command) {
            Ok(Command::Search {
                query,
                limit: 5,
                json: true,
                no_prefix: false,
                synonyms: None,
                ..
            }) => assert_eq!(query, ["kick", "-vinyl", "dark"]),
            other => panic!("{other:?}"),
        }
        let cli = Cli::try_parse_from(["sonar", "search", "--no-prefix", "-\"one shot\""]);
        assert!(matches!(
            cli.map(|c| c.command),
            Ok(Command::Search { ref query, no_prefix: true, .. }) if query == &["-\"one shot\""]
        ));
        let cli = Cli::try_parse_from(["sonar", "search"]);
        assert!(matches!(
            cli.map(|c| c.command),
            Ok(Command::Search { ref query, limit: 20, .. }) if query.is_empty()
        ));
        // From the first query word on, everything is query text.
        let cli = Cli::try_parse_from(["sonar", "search", "kick", "--json"]);
        assert!(matches!(
            cli.map(|c| c.command),
            Ok(Command::Search { ref query, json: false, .. }) if query == &["kick", "--json"]
        ));
        assert!(Cli::try_parse_from(["sonar", "index", "--json"]).is_ok());
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
