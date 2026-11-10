//! Filters on effective values (ADR-018) and their Tantivy queries.
//!
//! Semantics:
//! - Sets (categories, kinds, keys, libraries) are any-of; an empty set
//!   does not filter. Tags are all-of.
//! - Ranges (BPM, duration) are inclusive at both ends; an open end is
//!   `None`.
//! - A file whose value is unknown never matches an active filter on it.
//! - Hidden files are excluded unless [`Filters::include_hidden`].
//! - Strings match after the same folding as the index (lowercase, no
//!   diacritics): `--category Kick` = `kick`.

use std::ops::Bound;

use serde::Serialize;
use tantivy::query::{
    ConstScoreQuery, EmptyQuery, Occur, Query, RangeQuery, TermQuery, TermSetQuery,
};
use tantivy::schema::{Field, IndexRecordOption};
use tantivy::Term;

use super::keys::{compatible_codes, key_code};
use crate::index::{bpm_centi, facet_value, Fields};
use crate::text::Key;

/// An inclusive range; `None` leaves that end open.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct ValueRange<T> {
    /// Smallest accepted value.
    pub min: Option<T>,
    /// Largest accepted value.
    pub max: Option<T>,
}

impl<T> ValueRange<T> {
    /// `min..=max` with either end open.
    pub fn new(min: Option<T>, max: Option<T>) -> Self {
        Self { min, max }
    }
}

/// What to keep. `Filters::default()` keeps every visible file.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Filters {
    /// Effective category is one of these (`kick`, `bass`, ...).
    pub categories: Vec<String>,
    /// Effective kind is one of these (`loop`, `one_shot`; see
    /// [`normalize_kind`] for accepted spellings).
    pub kinds: Vec<String>,
    /// Effective tempo in BPM within this range.
    pub bpm: Option<ValueRange<f64>>,
    /// Effective key is one of these (mode `None` = major).
    pub keys: Vec<Key>,
    /// Also accept the harmonically compatible keys of [`Filters::keys`]
    /// (Camelot wheel, see [`super::keys`]).
    pub compatible_keys: bool,
    /// Duration in milliseconds within this range.
    pub duration_ms: Option<ValueRange<u64>>,
    /// In one of these libraries (`libraries.id`).
    pub libraries: Vec<i64>,
    /// Has every one of these tags (case-insensitive).
    pub tags: Vec<String>,
    /// Only favourites.
    pub favorites_only: bool,
    /// Also return hidden files (default: never).
    pub include_hidden: bool,
}

/// Canonical kind for a typed spelling: `one-shot`, `oneshot`, `one shot`,
/// `shot` → `one_shot`; `loops` → `loop`; anything else folded as is.
pub fn normalize_kind(raw: &str) -> String {
    let v = facet_value(raw);
    let compact: String = v.chars().filter(|c| c.is_alphanumeric()).collect();
    match compact.as_str() {
        "oneshot" | "oneshots" | "shot" | "shots" => "one_shot".into(),
        "loop" | "loops" => "loop".into(),
        _ => v,
    }
}

impl Filters {
    /// Key codes to accept, sorted and deduplicated (expanded to the
    /// compatible keys if asked); empty = no key filter.
    pub(crate) fn key_codes(&self) -> Vec<u64> {
        let mut codes: Vec<u64> = Vec::new();
        for &k in &self.keys {
            let code = key_code(k);
            if self.compatible_keys {
                codes.extend(compatible_codes(code).map(u64::from));
            } else {
                codes.push(u64::from(code));
            }
        }
        codes.sort_unstable();
        codes.dedup();
        codes
    }

    /// Folded category values.
    pub(crate) fn category_values(&self) -> Vec<String> {
        folded_set(self.categories.iter().map(|c| facet_value(c)))
    }

    /// Canonical kind values.
    pub(crate) fn kind_values(&self) -> Vec<String> {
        folded_set(self.kinds.iter().map(|k| normalize_kind(k)))
    }

    /// Library ids as index values (negative ids can match nothing and
    /// are mapped to an impossible value).
    pub(crate) fn library_values(&self) -> Vec<u64> {
        let mut v: Vec<u64> = self
            .libraries
            .iter()
            .map(|&id| u64::try_from(id).unwrap_or(u64::MAX))
            .collect();
        v.sort_unstable();
        v.dedup();
        v
    }

    /// Clauses that always restrict the matching set: ranges, tags,
    /// hidden. With `with_facet_dims` the facet dimensions (category,
    /// kind, key, library, favourites) are added too; without, the caller
    /// checks them per document (drill-sideways facet counting).
    pub(crate) fn clauses(
        &self,
        f: &Fields,
        with_facet_dims: bool,
    ) -> Vec<(Occur, Box<dyn Query>)> {
        let ff = &f.ff;
        let mut out: Vec<(Occur, Box<dyn Query>)> = Vec::new();
        let mut must = |q: Box<dyn Query>| out.push((Occur::Must, filter(q)));
        if let Some(r) = self.bpm {
            // Compared in hundredths, like the index (120.004 = 120.00). A
            // non-positive minimum is no bound; a non-positive maximum
            // keeps nothing.
            let lo = r.min.and_then(bpm_centi);
            let hi = r.max.map(|b| bpm_centi(b).unwrap_or(0));
            must(range(ff.bpm, lo, hi));
        }
        if let Some(r) = self.duration_ms {
            must(range(ff.duration, r.min, r.max));
        }
        for tag in folded_set(self.tags.iter().map(|t| facet_value(t))) {
            must(Box::new(TermQuery::new(
                Term::from_field_text(ff.tags, &tag),
                IndexRecordOption::Basic,
            )));
        }
        if with_facet_dims {
            let cats = self.category_values();
            if !cats.is_empty() {
                must(text_set(ff.category, &cats));
            }
            let kinds = self.kind_values();
            if !kinds.is_empty() {
                must(text_set(ff.kind, &kinds));
            }
            let keys = self.key_codes();
            if !keys.is_empty() {
                must(u64_set(ff.key, &keys));
            }
            let libs = self.library_values();
            if !libs.is_empty() {
                must(u64_set(ff.library, &libs));
            }
            if self.favorites_only {
                must(u64_set(ff.favorite, &[1]));
            }
        }
        if !self.include_hidden {
            out.push((Occur::MustNot, u64_set(ff.hidden, &[1])));
        }
        out
    }
}

/// Non-empty values, sorted and deduplicated.
fn folded_set(values: impl Iterator<Item = String>) -> Vec<String> {
    let mut v: Vec<String> = values.filter(|s| !s.is_empty()).collect();
    v.sort_unstable();
    v.dedup();
    v
}

/// A filter contributes no score (BM25 of the text decides the order).
fn filter(q: Box<dyn Query>) -> Box<dyn Query> {
    Box::new(ConstScoreQuery::new(q, 0.0))
}

/// `lo..=hi` on a u64 fast field; both open = every doc with a value
/// (so `ValueRange::default()` means "value is known").
fn range(field: Field, lo: Option<u64>, hi: Option<u64>) -> Box<dyn Query> {
    if let (Some(l), Some(h)) = (lo, hi) {
        if l > h {
            return Box::new(EmptyQuery);
        }
    }
    let lo = Bound::Included(Term::from_field_u64(field, lo.unwrap_or(0)));
    let hi = hi.map_or(Bound::Unbounded, |h| {
        Bound::Included(Term::from_field_u64(field, h))
    });
    Box::new(RangeQuery::new(lo, hi))
}

fn text_set(field: Field, values: &[String]) -> Box<dyn Query> {
    Box::new(TermSetQuery::new(
        values.iter().map(|v| Term::from_field_text(field, v)),
    ))
}

fn u64_set(field: Field, values: &[u64]) -> Box<dyn Query> {
    Box::new(TermSetQuery::new(
        values.iter().map(|&v| Term::from_field_u64(field, v)),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::keys::parse_key;

    #[test]
    fn kinds_normalize() {
        for (raw, want) in [
            ("one-shot", "one_shot"),
            ("One Shot", "one_shot"),
            ("oneshot", "one_shot"),
            ("one_shot", "one_shot"),
            ("Loops", "loop"),
            ("loop", "loop"),
            ("Weird", "weird"),
        ] {
            assert_eq!(normalize_kind(raw), want, "{raw}");
        }
    }

    #[test]
    fn key_codes_expand_and_dedup() {
        let key = |s| parse_key(s).expect("key");
        let mut f = Filters {
            keys: vec![key("Am"), key("C")],
            ..Filters::default()
        };
        assert_eq!(f.key_codes(), [0, 19]);
        f.compatible_keys = true;
        // Am → Am Dm Em C; C → C F G Am: union of six.
        let labels: Vec<String> = f
            .key_codes()
            .iter()
            .map(|&c| crate::search::keys::key_label(c as u8))
            .collect();
        assert_eq!(labels, ["C", "Dm", "Em", "F", "G", "Am"]);
    }
}
