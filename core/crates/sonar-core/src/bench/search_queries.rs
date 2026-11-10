//! The query mix of the search benchmark, one [`QueryClass`] per kind of
//! interaction. Every query asks for facet counts too (the app shows the
//! facet chips next to the results), so this is the costlier variant of
//! each interaction.

use serde::Serialize;

use crate::search::keys::parse_key;
use crate::search::{Filters, SearchOptions, Sort, SortField, ValueRange};
use crate::text::Key;

/// Kinds of interaction, reported separately.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryClass {
    /// Every keystroke of typed queries (unfinished last word = prefix),
    /// including synonyms (`bd`, `hh`) and exclusions (`snare -rim`).
    AsYouType,
    /// A text query plus filters (category, kind, BPM range, keys with
    /// Camelot-compatible expansion, library, tag, favourites, duration).
    TextFilters,
    /// Empty query (browse all) sorted by name, optionally with a facet
    /// selected.
    BrowseName,
    /// Empty query sorted by BPM (ascending and descending).
    BrowseBpm,
    /// Empty query with a BPM or duration range.
    RangeFacets,
}

impl QueryClass {
    /// Every class, in report order.
    pub const ALL: [Self; 5] = [
        Self::AsYouType,
        Self::TextFilters,
        Self::BrowseName,
        Self::BrowseBpm,
        Self::RangeFacets,
    ];

    /// Short name (`as_you_type`).
    pub fn name(self) -> &'static str {
        match self {
            Self::AsYouType => "as_you_type",
            Self::TextFilters => "text_filters",
            Self::BrowseName => "browse_name",
            Self::BrowseBpm => "browse_bpm",
            Self::RangeFacets => "range_facets",
        }
    }
}

/// One timed search.
#[derive(Debug, Clone)]
pub(crate) struct Case {
    /// Class the timing is reported under.
    pub class: QueryClass,
    /// Raw query text as typed (empty = browse all).
    pub query: String,
    /// Filters, sort and facet request sent with the query.
    pub opts: SearchOptions,
}

/// Typed char by char: every non-empty prefix is one query.
const TYPED: [&str; 12] = [
    "kick 12",
    "dark pad",
    "808 glide",
    "snare -rim",
    "bd punchy",
    "hh open",
    "vocal chop",
    "lofi drums loop",
    "c minor",
    "124 bpm",
    "trap essentials",
    "riser",
];

fn key(s: &str) -> Vec<Key> {
    parse_key(s).into_iter().collect()
}

fn bpm(min: Option<f64>, max: Option<f64>) -> Option<ValueRange<f64>> {
    Some(ValueRange::new(min, max))
}

fn strings(xs: &[&str]) -> Vec<String> {
    xs.iter().map(|s| (*s).to_owned()).collect()
}

/// The full mix; `libraries` are the corpus's library ids (filters use
/// the second one if there is one).
pub(crate) fn cases(libraries: &[i64]) -> Vec<Case> {
    let case = |class, query: &str, filters: Filters, sort: Sort| Case {
        class,
        query: query.to_owned(),
        opts: SearchOptions {
            filters,
            sort,
            facets: true,
            ..SearchOptions::default()
        },
    };
    let mut out = Vec::new();
    for phrase in TYPED {
        let ends = phrase.char_indices().map(|(i, c)| i + c.len_utf8());
        for end in ends {
            let q = &phrase[..end];
            out.push(case(
                QueryClass::AsYouType,
                q,
                Filters::default(),
                Sort::default(),
            ));
        }
    }

    let tf = QueryClass::TextFilters;
    let lib = libraries.get(1).or(libraries.first()).copied().unwrap_or(1);
    let d = Filters::default;
    let rel = Sort::default();
    out.extend([
        case(
            tf,
            "kick",
            Filters {
                categories: strings(&["kick"]),
                kinds: strings(&["one_shot"]),
                ..d()
            },
            rel,
        ),
        case(
            tf,
            "bass",
            Filters {
                kinds: strings(&["loop"]),
                bpm: bpm(Some(120.0), Some(130.0)),
                ..d()
            },
            rel,
        ),
        case(
            tf,
            "pad",
            Filters {
                keys: key("Am"),
                compatible_keys: true,
                ..d()
            },
            rel,
        ),
        case(
            tf,
            "dark",
            Filters {
                keys: key("Cm"),
                bpm: bpm(Some(85.0), Some(95.0)),
                ..d()
            },
            rel,
        ),
        case(
            tf,
            "vocal",
            Filters {
                libraries: vec![lib],
                ..d()
            },
            rel,
        ),
        case(
            tf,
            "loop",
            Filters {
                tags: strings(&["mix-ready"]),
                ..d()
            },
            rel,
        ),
        case(
            tf,
            "snare",
            Filters {
                favorites_only: true,
                ..d()
            },
            rel,
        ),
        case(
            tf,
            "hat",
            Filters {
                duration_ms: Some(ValueRange::new(None, Some(500))),
                ..d()
            },
            rel,
        ),
        case(
            tf,
            "synth lead",
            Filters {
                keys: key("F#m"),
                compatible_keys: true,
                kinds: strings(&["loop"]),
                ..d()
            },
            rel,
        ),
        case(
            tf,
            "perc",
            Filters {
                categories: strings(&["perc", "clap"]),
                ..d()
            },
            Sort::by(SortField::Bpm),
        ),
    ]);

    let bn = QueryClass::BrowseName;
    out.extend([
        case(bn, "", d(), Sort::by(SortField::Name)),
        case(bn, "", d(), Sort::desc(SortField::Name)),
        case(
            bn,
            "",
            Filters {
                categories: strings(&["kick"]),
                ..d()
            },
            Sort::by(SortField::Name),
        ),
    ]);
    let bb = QueryClass::BrowseBpm;
    out.extend([
        case(bb, "", d(), Sort::by(SortField::Bpm)),
        case(bb, "", d(), Sort::desc(SortField::Bpm)),
        case(
            bb,
            "",
            Filters {
                kinds: strings(&["loop"]),
                ..d()
            },
            Sort::by(SortField::Bpm),
        ),
    ]);
    let rf = QueryClass::RangeFacets;
    let name = Sort::by(SortField::Name);
    out.extend([
        case(
            rf,
            "",
            Filters {
                bpm: bpm(Some(118.0), Some(122.0)),
                ..d()
            },
            name,
        ),
        case(
            rf,
            "",
            Filters {
                bpm: bpm(Some(85.0), Some(95.0)),
                ..d()
            },
            name,
        ),
        case(
            rf,
            "",
            Filters {
                bpm: bpm(Some(140.0), Some(175.0)),
                ..d()
            },
            Sort::by(SortField::Bpm),
        ),
        case(
            rf,
            "",
            Filters {
                bpm: bpm(Some(170.0), None),
                ..d()
            },
            name,
        ),
        case(
            rf,
            "",
            Filters {
                duration_ms: Some(ValueRange::new(None, Some(2_000))),
                ..d()
            },
            name,
        ),
        case(
            rf,
            "",
            Filters {
                bpm: bpm(Some(120.0), Some(128.0)),
                kinds: strings(&["loop"]),
                ..d()
            },
            name,
        ),
    ]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mix_covers_every_class_and_types_prefixes() {
        let cases = cases(&[1, 2]);
        for class in QueryClass::ALL {
            assert!(cases.iter().any(|c| c.class == class), "{class:?}");
        }
        let typed: Vec<&str> = cases
            .iter()
            .filter(|c| c.class == QueryClass::AsYouType)
            .map(|c| c.query.as_str())
            .take(7)
            .collect();
        assert_eq!(
            typed,
            ["k", "ki", "kic", "kick", "kick ", "kick 1", "kick 12"]
        );
        assert!(cases.iter().all(|c| c.opts.facets));
    }
}
