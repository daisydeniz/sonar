//! Result order (ADR-018). Files whose sort value is unknown always come
//! last, in both directions; ties go by file id (stable across queries).

use std::fmt;
use std::str::FromStr;

use serde::Serialize;

/// What to order results by.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SortField {
    /// Text relevance (best first). Without query text there is no
    /// relevance: results are ordered by [`SortField::Name`] instead.
    #[default]
    Relevance,
    /// File name (folded: case- and accent-insensitive, byte order).
    Name,
    /// Effective tempo.
    Bpm,
    /// Effective key, by root then mode: `C`, `Cm`, `C#`, `C#m`, …, `Bm`.
    Key,
    /// Duration.
    Duration,
    /// Effective category (alphabetical).
    Category,
    /// Effective kind (`loop` before `one_shot`).
    Kind,
    /// Library id (order the libraries were added).
    Library,
}

impl SortField {
    const NAMES: [(&'static str, Self); 8] = [
        ("relevance", Self::Relevance),
        ("name", Self::Name),
        ("bpm", Self::Bpm),
        ("key", Self::Key),
        ("duration", Self::Duration),
        ("category", Self::Category),
        ("kind", Self::Kind),
        ("library", Self::Library),
    ];

    /// Lowercase name (`bpm`), as accepted by [`Sort::from_str`].
    pub fn name(self) -> &'static str {
        Self::NAMES
            .iter()
            .find(|(_, f)| *f == self)
            .map_or("relevance", |(n, _)| n)
    }
}

/// Field and direction. `descending` is ignored for relevance (always
/// best first).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Sort {
    /// What to order by.
    pub field: SortField,
    /// Largest / last first.
    pub descending: bool,
}

impl Sort {
    /// Ascending order by `field`.
    pub fn by(field: SortField) -> Self {
        Self {
            field,
            descending: false,
        }
    }

    /// Descending order by `field`.
    pub fn desc(field: SortField) -> Self {
        Self {
            field,
            descending: true,
        }
    }
}

/// A sort spec that is not `<field>[:asc|:desc]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseSortError(String);

impl fmt::Display for ParseSortError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names: Vec<&str> = SortField::NAMES.iter().map(|(n, _)| *n).collect();
        write!(
            f,
            "bad sort {:?}: expected <field>[:asc|:desc], field one of {}",
            self.0,
            names.join(", ")
        )
    }
}

impl std::error::Error for ParseSortError {}

impl FromStr for Sort {
    type Err = ParseSortError;

    /// `bpm`, `bpm:desc`, `name:asc` (case-insensitive).
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let err = || ParseSortError(s.to_owned());
        let lower = s.trim().to_ascii_lowercase();
        let (name, dir) = lower.split_once(':').unwrap_or((&lower, "asc"));
        let descending = match dir {
            "asc" => false,
            "desc" => true,
            _ => return Err(err()),
        };
        let field = SortField::NAMES
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, f)| *f)
            .ok_or_else(err)?;
        Ok(Self { field, descending })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_specs() {
        assert_eq!("bpm".parse(), Ok(Sort::by(SortField::Bpm)));
        assert_eq!("BPM:desc".parse(), Ok(Sort::desc(SortField::Bpm)));
        assert_eq!("name:asc".parse(), Ok(Sort::by(SortField::Name)));
        assert_eq!("relevance".parse(), Ok(Sort::default()));
        for bad in ["", "bpm:up", "tempo", "bpm:desc:x"] {
            assert!(bad.parse::<Sort>().is_err(), "{bad}");
        }
        for (n, f) in SortField::NAMES {
            assert_eq!(f.name(), n);
        }
    }
}
