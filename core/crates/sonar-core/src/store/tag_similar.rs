//! Similar-tag suggestions (T12.2, ADR-043): pure functions, no database.
//!
//! Two (normalised) tags are *similar* when, comparing case-insensitively:
//! 1. they are equal ignoring `-`, `_` and spaces (`lofi` / `lo-fi`), or
//! 2. both have at least 4 characters and are one Damerau-Levenshtein edit
//!    apart (insert, delete, substitute, or swap two neighbours:
//!    `groove` / `grove`, `kick` / `kicks`), or
//! 3. one is a prefix of the other, the shorter has at least 5 characters
//!    and the longer adds at most 2 (`analog` / `analogue`).
//!
//! The first rule that matches is the reported [`SimilarReason`]. Pairs are
//! only suggestions; the caller drops the ones the user has ignored.
//!
//! Speed: rule 1 is a hash grouping; rules 2 and 3 only look at tags whose
//! length differs by at most 2 (names are sorted by length), and distance 1
//! is decided by a linear scan after a first/last-character prefilter, so
//! 5,000 tags stay far below the 200 ms budget (see the test).

use std::collections::{HashMap, HashSet};

use serde::Serialize;

/// Why two tags were suggested as the same.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SimilarReason {
    /// Equal once `-`, `_` and spaces are removed.
    Separators,
    /// One edit apart (Damerau-Levenshtein distance 1).
    Typo,
    /// One is the other plus one or two letters.
    Prefix,
}

/// Two similar tags with their file counts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SimilarPair {
    /// First tag (the one that sorts first, ignoring case).
    pub a: String,
    /// Files carrying `a`.
    pub a_files: i64,
    /// Second tag.
    pub b: String,
    /// Files carrying `b`.
    pub b_files: i64,
    /// Which rule matched.
    pub reason: SimilarReason,
}

/// Lower-cased, order-independent key of a pair, as stored for ignored
/// pairs: `(a, b)` with `a < b`.
pub fn pair_key(x: &str, y: &str) -> (String, String) {
    let (x, y) = (x.to_lowercase(), y.to_lowercase());
    if x <= y {
        (x, y)
    } else {
        (y, x)
    }
}

/// Which rule makes `a` and `b` (already lower-cased) similar, if any.
#[cfg(test)]
fn similar_reason(a: &[char], b: &[char]) -> Option<SimilarReason> {
    if a == b {
        return None;
    }
    if strip_separators(a) == strip_separators(b) {
        return Some(SimilarReason::Separators);
    }
    window_reason(a, b)
}

/// Rules 2 and 3 (the ones that need similar lengths).
fn window_reason(a: &[char], b: &[char]) -> Option<SimilarReason> {
    let (short, long) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    let extra = long.len() - short.len();
    if extra > 2 {
        return None;
    }
    if short.len() >= 4 && within_one_edit(short, long) {
        return Some(SimilarReason::Typo);
    }
    if short.len() >= 5 && extra >= 1 && long.starts_with(short) {
        return Some(SimilarReason::Prefix);
    }
    None
}

fn strip_separators(s: &[char]) -> Vec<char> {
    s.iter()
        .copied()
        .filter(|c| !matches!(c, '-' | '_' | ' '))
        .collect()
}

/// True when `a` and `b` differ by exactly one insert, delete, substitution
/// or adjacent swap (`a != b` is assumed; `short.len() <= long.len()`).
fn within_one_edit(short: &[char], long: &[char]) -> bool {
    // One edit keeps the first or the last character in place.
    if short.first() != long.first() && short.last() != long.last() {
        return false;
    }
    let prefix = short.iter().zip(long).take_while(|(x, y)| x == y).count();
    match long.len() - short.len() {
        0 => {
            // Substitution, or a swap of the two characters at `prefix`.
            let rest_s = &short[prefix + 1..];
            let rest_l = &long[prefix + 1..];
            if rest_s == rest_l {
                return true;
            }
            prefix + 1 < short.len()
                && short[prefix] == long[prefix + 1]
                && short[prefix + 1] == long[prefix]
                && short[prefix + 2..] == long[prefix + 2..]
        }
        1 => short[prefix..] == long[prefix + 1..],
        _ => false,
    }
}

/// All similar pairs among `tags` (`(name, files)`, normalised names),
/// minus the `ignored` pairs (keys from [`pair_key`]). Sorted by the first,
/// then the second name (ignoring case).
pub fn similar_pairs(
    tags: &[(String, i64)],
    ignored: &HashSet<(String, String)>,
) -> Vec<SimilarPair> {
    let chars: Vec<Vec<char>> = tags
        .iter()
        .map(|(n, _)| n.to_lowercase().chars().collect())
        .collect();
    // (i, j) with i < j  ->  reason; the first rule found wins.
    let mut found: HashMap<(usize, usize), SimilarReason> = HashMap::new();

    // Rule 1: group by the separator-free form.
    let mut groups: HashMap<Vec<char>, Vec<usize>> = HashMap::new();
    for (i, c) in chars.iter().enumerate() {
        groups.entry(strip_separators(c)).or_default().push(i);
    }
    for members in groups.values().filter(|m| m.len() > 1) {
        for (k, &i) in members.iter().enumerate() {
            for &j in &members[k + 1..] {
                found.insert((i.min(j), i.max(j)), SimilarReason::Separators);
            }
        }
    }

    // Rules 2 and 3: sorted by length, compare within a +2 window.
    let mut by_len: Vec<usize> = (0..tags.len()).collect();
    by_len.sort_by_key(|&i| chars[i].len());
    for (pos, &i) in by_len.iter().enumerate() {
        if chars[i].len() < 4 {
            continue; // both rules need a shorter side of >= 4
        }
        for &j in &by_len[pos + 1..] {
            if chars[j].len() > chars[i].len() + 2 {
                break;
            }
            if chars[i] == chars[j] {
                continue;
            }
            if let Some(reason) = window_reason(&chars[i], &chars[j]) {
                found.entry((i.min(j), i.max(j))).or_insert(reason);
            }
        }
    }

    let mut out: Vec<SimilarPair> = found
        .into_iter()
        .filter(|&((i, j), _)| !ignored.contains(&pair_key(&tags[i].0, &tags[j].0)))
        .map(|((i, j), reason)| {
            let (x, y) = if chars[i] <= chars[j] { (i, j) } else { (j, i) };
            SimilarPair {
                a: tags[x].0.clone(),
                a_files: tags[x].1,
                b: tags[y].0.clone(),
                b_files: tags[y].1,
                reason,
            }
        })
        .collect();
    out.sort_by_cached_key(|p| (p.a.to_lowercase(), p.b.to_lowercase()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use SimilarReason::{Prefix, Separators, Typo};

    fn reason(a: &str, b: &str) -> Option<SimilarReason> {
        let a: Vec<char> = a.to_lowercase().chars().collect();
        let b: Vec<char> = b.to_lowercase().chars().collect();
        similar_reason(&a, &b)
    }

    #[test]
    fn rule_table() {
        let table: &[(&str, &str, Option<SimilarReason>)] = &[
            ("groove", "grove", Some(Typo)),
            ("lofi", "lo-fi", Some(Separators)),
            ("lo fi", "lo_fi", Some(Separators)),
            ("analog", "analogue", Some(Prefix)),
            ("kick", "kicks", Some(Typo)),
            ("Kick", "kicks", Some(Typo)),
            ("snare", "snrae", Some(Typo)), // swap
            ("bass", "boss", Some(Typo)),   // substitution
            ("hat", "hats", None),          // under 4 chars, prefix under 5
            ("drum", "bass", None),
            ("drums", "drummer", None), // adds 2 but is not a prefix of
            ("guitar", "guitars", Some(Typo)),
            ("synth", "synthwave", None), // adds 4
            ("pluck", "plucked", Some(Prefix)),
            ("bell", "bells", Some(Typo)),
            ("kick", "kick", None),
            ("abc", "abd", None), // under 4 chars
            ("dark", "dork", Some(Typo)),
            ("dark", "dirk x", None),
        ];
        for &(a, b, want) in table {
            assert_eq!(reason(a, b), want, "{a} / {b}");
            assert_eq!(reason(b, a), want, "{b} / {a}");
        }
    }

    fn tags(names: &[&str]) -> Vec<(String, i64)> {
        names.iter().map(|n| ((*n).to_owned(), 1)).collect()
    }

    #[test]
    fn pairs_skip_ignored_in_either_order_and_case() {
        let t = tags(&["Groove", "grove", "lofi", "lo-fi", "drum", "bass"]);
        let all = similar_pairs(&t, &HashSet::new());
        let shown: Vec<_> = all.iter().map(|p| (p.a.as_str(), p.b.as_str())).collect();
        assert_eq!(shown, [("Groove", "grove"), ("lo-fi", "lofi")]);
        let mut ignored = HashSet::new();
        ignored.insert(pair_key("GROVE", "groove"));
        let left = similar_pairs(&t, &ignored);
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].reason, Separators);
    }

    #[test]
    fn separators_win_over_typo_and_pair_appears_once() {
        let all = similar_pairs(&tags(&["lofi", "lo-fi"]), &HashSet::new());
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].reason, Separators);
    }

    /// 5,000 plausible tags: ~200 ms budget in release (ADR-043); debug
    /// builds are an order of magnitude slower, so the guard there is loose.
    #[test]
    fn five_thousand_tags_are_fast() {
        let syll = [
            "ka", "lo", "mi", "ra", "ten", "bo", "su", "fi", "gre", "dum", "pa", "ze", "ni", "tor",
        ];
        // Mixed-radix counter over 4 syllables: 14^4 distinct names.
        let t: Vec<(String, i64)> = (0..5000usize)
            .map(|n| {
                let d = |k: u32| syll[(n / 14usize.pow(k)) % 14];
                (format!("{}{}{}{}", d(0), d(1), d(2), d(3)), 3)
            })
            .collect();
        let start = std::time::Instant::now();
        let pairs = similar_pairs(&t, &HashSet::new());
        let took = start.elapsed();
        eprintln!("similar over 5000 tags: {took:?}, {} pairs", pairs.len());
        let budget = if cfg!(debug_assertions) { 5000 } else { 200 };
        assert!(took.as_millis() < budget, "{took:?}");
    }
}
