//! `sonar search` filter, sort and facet flags (ADR-018), flattened into
//! the `search` subcommand. Like every search option they go before the
//! query words.

use anyhow::{bail, Context, Result};
use clap::Args;
use sonar_core::search::keys::parse_key;
use sonar_core::search::{Filters, Sort, ValueRange};
use sonar_core::text::Key;
use sonar_core::Store;

/// Filters, sort and facets for `sonar search`.
#[derive(Debug, Clone, Default, Args)]
pub struct FilterArgs {
    /// Only these categories (any of; comma-separated or repeated).
    #[arg(long, value_name = "NAME", value_delimiter = ',')]
    pub category: Vec<String>,
    /// Only these kinds: `loop`, `one-shot` (any of).
    #[arg(long, value_name = "KIND", value_delimiter = ',')]
    pub kind: Vec<String>,
    /// BPM range, inclusive: `120-128`, `120-` (at least), `-128` (at
    /// most), `128` (exactly). Files with unknown BPM are left out.
    #[arg(long, value_name = "RANGE", allow_hyphen_values = true, value_parser = parse_bpm)]
    pub bpm: Option<ValueRange<f64>>,
    /// Only these keys (any of): `Am`, `C#`, `Bb minor`, Camelot `8A`.
    #[arg(long, value_name = "KEY", value_delimiter = ',', value_parser = parse_key_arg)]
    pub key: Vec<Key>,
    /// With `--key`: also the harmonically compatible keys (Camelot
    /// wheel: ±1 and relative major/minor).
    #[arg(long, requires = "key")]
    pub compatible: bool,
    /// Duration range in seconds (`0-2`, `0.5-`, `-1.5`), or in
    /// milliseconds with an `ms` suffix (`0-2000ms`). Inclusive.
    #[arg(long, value_name = "RANGE", allow_hyphen_values = true, value_parser = parse_duration)]
    pub duration: Option<ValueRange<u64>>,
    /// Only these libraries, by id or root path (any of).
    #[arg(long, value_name = "ID|PATH")]
    pub library: Vec<String>,
    /// Only files with this tag (repeat for several: all of them).
    #[arg(long, value_name = "TAG")]
    pub tag: Vec<String>,
    /// Only favourites.
    #[arg(long)]
    pub favorites: bool,
    /// Also show files the user hid.
    #[arg(long)]
    pub include_hidden: bool,
    /// Order: relevance (default; by name without query text), name, bpm,
    /// key, duration, category, kind, library; `:desc` reverses
    /// (`bpm:desc`). Unknown values always come last.
    #[arg(long, value_name = "FIELD[:desc]")]
    pub sort: Option<Sort>,
    /// Also print facet counts (category, kind, key, library, tag,
    /// favourites).
    #[arg(long)]
    pub facets: bool,
}

impl FilterArgs {
    /// The core filters; library paths are resolved in `store`.
    pub fn filters(&self, store: &Store) -> Result<Filters> {
        let mut libraries = Vec::new();
        for lib in &self.library {
            let id = match lib.parse::<i64>() {
                Ok(id) => id,
                Err(_) => store
                    .library_by_root(lib)
                    .context("looking up the library")?
                    .map(|l| l.id)
                    .with_context(|| format!("no library at {lib}"))?,
            };
            libraries.push(id);
        }
        Ok(Filters {
            categories: self.category.clone(),
            kinds: self.kind.clone(),
            bpm: self.bpm,
            keys: self.key.clone(),
            compatible_keys: self.compatible,
            duration_ms: self.duration,
            libraries,
            tags: self.tag.clone(),
            favorites_only: self.favorites,
            include_hidden: self.include_hidden,
        })
    }
}

fn parse_key_arg(s: &str) -> Result<Key, String> {
    parse_key(s).ok_or_else(|| format!("not a key: {s:?} (try Am, C#, Bb minor, 8A)"))
}

/// `A-B`, `A-`, `-B` or `A` (= `A-A`) as numbers ≥ 0.
fn parse_range(s: &str) -> Result<ValueRange<f64>> {
    let (a, b) = s.split_once('-').unwrap_or((s, s));
    let num = |t: &str| -> Result<Option<f64>> {
        let t = t.trim();
        if t.is_empty() {
            return Ok(None);
        }
        match t.parse::<f64>() {
            Ok(v) if v.is_finite() && v >= 0.0 => Ok(Some(v)),
            _ => bail!("not a number ≥ 0: {t:?}"),
        }
    };
    let r = ValueRange::new(num(a)?, num(b)?);
    if r.min.is_none() && r.max.is_none() {
        bail!("empty range {s:?} (use A-B, A-, -B or A)");
    }
    Ok(r)
}

fn parse_bpm(s: &str) -> Result<ValueRange<f64>, String> {
    parse_range(s.trim()).map_err(|e| e.to_string())
}

fn parse_duration(s: &str) -> Result<ValueRange<u64>, String> {
    let s = s.trim();
    let (body, ms_per_unit) = match s.strip_suffix("ms") {
        Some(b) => (b, 1.0),
        None => (s.strip_suffix('s').unwrap_or(s), 1000.0),
    };
    let r = parse_range(body).map_err(|e| e.to_string())?;
    // Non-negative and finite (checked above); far beyond any real
    // duration before the cast could saturate.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let ms = |v: f64| (v * ms_per_unit).round() as u64;
    Ok(ValueRange::new(r.min.map(ms), r.max.map(ms)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges() {
        let r = |s| parse_bpm(s).map(|r| (r.min, r.max));
        assert_eq!(r("120-128"), Ok((Some(120.0), Some(128.0))));
        assert_eq!(r("120-"), Ok((Some(120.0), None)));
        assert_eq!(r("-128"), Ok((None, Some(128.0))));
        assert_eq!(r("92.5"), Ok((Some(92.5), Some(92.5))));
        for bad in ["", "-", "fast", "1-2-3", "nan"] {
            assert!(parse_bpm(bad).is_err(), "{bad}");
        }
        let d = |s| parse_duration(s).map(|r| (r.min, r.max));
        assert_eq!(d("0-2"), Ok((Some(0), Some(2000))));
        assert_eq!(d("0.5-"), Ok((Some(500), None)));
        assert_eq!(d("-1.5s"), Ok((None, Some(1500))));
        assert_eq!(d("0-250ms"), Ok((Some(0), Some(250))));
    }

    #[test]
    fn parses_filter_flags_before_the_words() {
        use crate::cli::{Cli, Command};
        use clap::Parser;
        use sonar_core::search::SortField;
        // A hyphen range is a value, not a flag.
        let cli = Cli::try_parse_from([
            "sonar",
            "search",
            "--category",
            "kick,snare",
            "--bpm",
            "-128",
            "--key",
            "Am",
            "--compatible",
            "--tag",
            "dark",
            "--tag",
            "punchy",
            "--sort",
            "bpm:desc",
            "--facets",
            "dark",
            "--json",
        ]);
        match cli.map(|c| c.command) {
            Ok(Command::Search {
                query,
                filters: f,
                json: false,
                ..
            }) => {
                assert_eq!(query, ["dark", "--json"]);
                assert_eq!(f.category, ["kick", "snare"]);
                assert_eq!(f.bpm.and_then(|r| r.max), Some(128.0));
                assert_eq!(f.tag, ["dark", "punchy"]);
                assert!(f.compatible && f.facets);
                assert_eq!(f.sort, Some(Sort::desc(SortField::Bpm)));
            }
            other => panic!("{other:?}"),
        }
        assert!(Cli::try_parse_from(["sonar", "search", "--compatible", "x"]).is_err());
        assert!(Cli::try_parse_from(["sonar", "search", "--sort", "tempo"]).is_err());
    }

    #[test]
    fn keys() {
        assert!(parse_key_arg("Am").is_ok());
        assert!(parse_key_arg("8A").is_ok());
        assert!(parse_key_arg("kick").is_err());
    }
}
