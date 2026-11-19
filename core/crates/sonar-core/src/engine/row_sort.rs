//! Field order of display rows that are already in memory (a similarity
//! result: the nearest N files shown by BPM, key, name, ...).
//!
//! Same rules as the text index sort ([`crate::search::Sort`]): the values
//! compare like the index sort columns (folded name, key code, BPM in
//! hundredths), unknown values come last in both directions. The sort is
//! stable, so equal values keep the order they came in (nearest first).

use std::cmp::Ordering;

use super::FileRow;
use crate::index::{bpm_centi, facet_value};
use crate::search::keys::{key_code, parse_key};
use crate::search::{Sort, SortField};
use crate::text::fold;

/// A known sort value (never mixed within one sort).
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
enum SortValue {
    Num(u64),
    Str(String),
}

/// The value `row` sorts by, `None` when unknown.
fn value(row: &FileRow, field: SortField) -> Option<SortValue> {
    let text = |v: &Option<String>| {
        let v = facet_value(v.as_deref()?);
        (!v.is_empty()).then_some(SortValue::Str(v))
    };
    match field {
        SortField::Relevance => None,
        SortField::Name => Some(SortValue::Str(fold(&row.name))),
        SortField::Category => text(&row.category),
        SortField::Kind => text(&row.kind),
        SortField::Bpm => row.bpm.and_then(bpm_centi).map(SortValue::Num),
        SortField::Key => {
            let key = parse_key(row.key.as_deref()?)?;
            Some(SortValue::Num(u64::from(key_code(key))))
        }
        SortField::Duration => row.duration_ms.map(SortValue::Num),
        SortField::Library => u64::try_from(row.library_id).ok().map(SortValue::Num),
    }
}

/// `items` ordered by `sort` over the row each one carries (`row`).
/// [`SortField::Relevance`] keeps the given order.
pub fn sort_by_row<T>(items: Vec<T>, sort: Sort, row: impl Fn(&T) -> &FileRow) -> Vec<T> {
    if sort.field == SortField::Relevance {
        return items;
    }
    let mut keyed: Vec<(Option<SortValue>, T)> = items
        .into_iter()
        .map(|item| (value(row(&item), sort.field), item))
        .collect();
    keyed.sort_by(|(a, _), (b, _)| match (a, b) {
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
        (Some(a), Some(b)) if sort.descending => b.cmp(a),
        (Some(a), Some(b)) => a.cmp(b),
    });
    keyed.into_iter().map(|(_, item)| item).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: i64, name: &str, bpm: Option<f64>, key: Option<&str>) -> FileRow {
        FileRow {
            file_id: id,
            library_id: 1,
            path: name.into(),
            name: name.into(),
            folders: String::new(),
            category: None,
            kind: None,
            bpm,
            key: key.map(str::to_owned),
            key_conf: None,
            key_source: None,
            key_name: None,
            key_audio: None,
            note_name: None,
            note_cents: None,
            duration_ms: None,
            sample_rate: None,
            channels: None,
            preview_gain_db: None,
            favorite: false,
            hidden: false,
            tags: Vec::new(),
        }
    }

    fn ids(rows: Vec<FileRow>, sort: Sort) -> Vec<i64> {
        sort_by_row(rows, sort, |r| r)
            .iter()
            .map(|r| r.file_id)
            .collect()
    }

    fn rows() -> Vec<FileRow> {
        vec![
            row(1, "b", Some(140.0), None),
            row(2, "A", None, Some("Am")),
            row(3, "c", Some(90.0), Some("C")),
            row(4, "d", Some(140.0), None),
        ]
    }

    #[test]
    fn relevance_keeps_the_given_order() {
        assert_eq!(ids(rows(), Sort::by(SortField::Relevance)), [1, 2, 3, 4]);
        assert_eq!(ids(rows(), Sort::desc(SortField::Relevance)), [1, 2, 3, 4]);
    }

    #[test]
    fn unknown_last_both_ways_and_ties_keep_their_order() {
        assert_eq!(ids(rows(), Sort::by(SortField::Bpm)), [3, 1, 4, 2]);
        assert_eq!(ids(rows(), Sort::desc(SortField::Bpm)), [1, 4, 3, 2]);
        // C (code 0) before A minor; rows without a key stay nearest-first.
        assert_eq!(ids(rows(), Sort::by(SortField::Key)), [3, 2, 1, 4]);
        assert_eq!(ids(rows(), Sort::desc(SortField::Key)), [2, 3, 1, 4]);
    }

    #[test]
    fn names_compare_folded() {
        assert_eq!(ids(rows(), Sort::by(SortField::Name)), [2, 1, 3, 4]);
        assert_eq!(ids(rows(), Sort::desc(SortField::Name)), [4, 3, 1, 2]);
    }
}
