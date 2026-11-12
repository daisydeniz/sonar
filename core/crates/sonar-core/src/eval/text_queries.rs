//! Text-query sets with graded relevance for the text / NL search metrics
//! ([`super::text_search`], docs/EVAL.md "Text / NL search").
//!
//! - **Fixtures**: [`FIXTURE_QUERIES`], judged from the manifest truth.
//!   A query names categories and optionally kind / BPM range / key; a
//!   file of a named category is relevant with grade 2 when it also
//!   matches every other constraint, else grade 1. A query without a
//!   category judges by its constraints alone (grade 1).
//! - **Golden**: `$SONAR_GOLDEN_DIR/text_queries.jsonl`, one JSON object
//!   per line: `{"query": "dark kick", "relevant": {"Kicks/k1.wav": 2,
//!   "Kicks/k2.wav": 1}}` (paths relative to the golden directory or
//!   absolute; grades 1–3, higher = more relevant). Files it names that
//!   `labels.csv` does not are analysed too and join the pools.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::classify::{Category, Kind};
use crate::text::{Key, Mode};

use super::dataset::{Dataset, Item, Labels};

/// Query file inside the golden directory.
pub const TEXT_QUERIES_FILE: &str = "text_queries.jsonl";
/// Highest relevance grade.
pub const MAX_GRADE: u8 = 3;

/// One query and its judged files (paths joined with the dataset root).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextQuery {
    /// What the user types.
    pub query: String,
    /// `(file, grade ≥ 1)`.
    pub relevant: Vec<(PathBuf, u8)>,
}

/// A fixture query, judged from truth labels (module docs).
#[derive(Debug, Clone, Copy)]
pub struct Spec {
    /// Query text.
    pub query: &'static str,
    /// Any of these categories (empty = no category constraint).
    pub categories: &'static [Category],
    /// Kind constraint.
    pub kind: Option<Kind>,
    /// Inclusive BPM range.
    pub bpm: Option<(f64, f64)>,
    /// Key constraint (root pitch class, mode).
    pub key: Option<(u8, Mode)>,
}

const fn q(query: &'static str, categories: &'static [Category]) -> Spec {
    Spec {
        query,
        categories,
        kind: None,
        bpm: None,
        key: None,
    }
}

const DRUM_HITS: &[Category] = &[Category::Kick, Category::Snare, Category::Hat];

/// The fixture query set: category words, synonyms, descriptive and
/// natural-language phrasings, and kind / tempo / key qualifiers.
pub const FIXTURE_QUERIES: &[Spec] = &[
    q("kick", &[Category::Kick]),
    q("punchy kick drum", &[Category::Kick]),
    q("bd", &[Category::Kick]),
    q("snare", &[Category::Snare]),
    q("snappy snare hit", &[Category::Snare]),
    q("hihat", &[Category::Hat]),
    q("closed hi hat", &[Category::Hat]),
    q("drum loop", &[Category::Drums]),
    Spec {
        bpm: Some((118.0, 122.0)),
        ..q("drum loop 120 bpm", &[Category::Drums])
    },
    q("breakbeat with kick and snare", &[Category::Drums]),
    q("pad", &[Category::Pad]),
    q("soft sustained synth pad", &[Category::Pad]),
    q("pluck", &[Category::Pluck]),
    Spec {
        kind: Some(Kind::OneShot),
        ..q("plucked synth one shot", &[Category::Pluck])
    },
    q("synth chords", &[Category::Synth]),
    Spec {
        key: Some((9, Mode::Minor)),
        ..q("chord loop in a minor", &[Category::Synth])
    },
    Spec {
        kind: Some(Kind::OneShot),
        ..q("drum one shot", DRUM_HITS)
    },
    Spec {
        kind: Some(Kind::Loop),
        ..q("loop", &[])
    },
    Spec {
        kind: Some(Kind::OneShot),
        ..q("one shot", &[])
    },
];

/// Grade of a file with `labels` for `spec` (0 = not relevant).
pub fn grade(spec: &Spec, labels: &Labels) -> u8 {
    let extras_ok = spec.kind.is_none_or(|k| labels.kind == Some(k))
        && spec
            .bpm
            .is_none_or(|(lo, hi)| labels.bpm.is_some_and(|b| (lo..=hi).contains(&b)))
        && spec.key.is_none_or(|(root, mode)| {
            labels.key
                == Some(Key {
                    root,
                    mode: Some(mode),
                })
        });
    if spec.categories.is_empty() {
        return u8::from(extras_ok);
    }
    if !labels
        .category
        .is_some_and(|c| spec.categories.contains(&c))
    {
        return 0;
    }
    if extras_ok {
        2
    } else {
        1
    }
}

/// [`FIXTURE_QUERIES`] judged over the fixture items of `items`.
pub fn fixture_queries(items: &[Item]) -> Vec<TextQuery> {
    FIXTURE_QUERIES
        .iter()
        .map(|spec| TextQuery {
            query: spec.query.to_owned(),
            relevant: items
                .iter()
                .filter(|i| !i.labels.expect_error)
                .filter_map(|i| {
                    let g = grade(spec, &i.labels);
                    (g > 0).then(|| (i.path.clone(), g))
                })
                .collect(),
        })
        .collect()
}

#[derive(serde::Deserialize)]
struct Line {
    query: String,
    relevant: std::collections::BTreeMap<String, u8>,
}

/// Parses `text_queries.jsonl` (module docs); bad lines become warnings.
pub fn parse_text_queries(text: &str, dir: &Path) -> (Vec<TextQuery>, Vec<String>) {
    let mut out = Vec::new();
    let mut warnings = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        match serde_json::from_str::<Line>(line) {
            Ok(l) if l.query.trim().is_empty() => {
                warnings.push(format!("{TEXT_QUERIES_FILE}:{}: empty query", n + 1));
            }
            Ok(l) => {
                let mut relevant = Vec::new();
                for (path, g) in l.relevant {
                    if (1..=MAX_GRADE).contains(&g) {
                        relevant.push((dir.join(path), g));
                    } else {
                        warnings.push(format!(
                            "{TEXT_QUERIES_FILE}:{}: grade {g} of {path:?} not in 1..={MAX_GRADE}",
                            n + 1
                        ));
                    }
                }
                out.push(TextQuery {
                    query: l.query,
                    relevant,
                });
            }
            Err(e) => warnings.push(format!("{TEXT_QUERIES_FILE}:{}: {e}", n + 1)),
        }
    }
    (out, warnings)
}

/// Reads `dir/text_queries.jsonl` into `ds` if present; files it names
/// that are not items join `ds.similar_extra` (analysed for the pools).
pub fn load_text_queries(ds: &mut Dataset, dir: &Path) {
    let path = dir.join(TEXT_QUERIES_FILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
        Err(e) => {
            ds.warnings.push(format!("{}: {e}", path.display()));
            return;
        }
    };
    let (queries, warnings) = parse_text_queries(&text, dir);
    ds.warnings.extend(warnings);
    let mut known: HashSet<PathBuf> = ds
        .items
        .iter()
        .chain(&ds.similar_extra)
        .map(|i| i.path.clone())
        .collect();
    for (p, _) in queries.iter().flat_map(|q| &q.relevant) {
        if p.is_file() && known.insert(p.clone()) {
            let id = p
                .strip_prefix(dir)
                .unwrap_or(p)
                .to_string_lossy()
                .into_owned();
            ds.similar_extra.push(Item {
                id,
                path: p.clone(),
                labels: Labels::default(),
            });
        }
    }
    ds.text_queries = queries;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(category: Option<Category>, kind: Option<Kind>, bpm: Option<f64>) -> Labels {
        Labels {
            category,
            kind,
            bpm,
            ..Labels::default()
        }
    }

    #[test]
    fn grades_follow_the_constraints() {
        let loop_120 = FIXTURE_QUERIES
            .iter()
            .find(|s| s.query == "drum loop 120 bpm")
            .expect("spec");
        let drums = Some(Category::Drums);
        assert_eq!(
            grade(loop_120, &labels(drums, Some(Kind::Loop), Some(120.0))),
            2
        );
        assert_eq!(
            grade(loop_120, &labels(drums, Some(Kind::Loop), Some(90.0))),
            1
        );
        assert_eq!(grade(loop_120, &labels(drums, None, None)), 1);
        assert_eq!(
            grade(loop_120, &labels(Some(Category::Kick), None, Some(120.0))),
            0
        );
        let kick = &FIXTURE_QUERIES[0];
        assert_eq!(grade(kick, &labels(Some(Category::Kick), None, None)), 2);
        assert_eq!(grade(kick, &labels(None, None, None)), 0);
        let any_loop = FIXTURE_QUERIES
            .iter()
            .find(|s| s.query == "loop")
            .expect("loop");
        assert_eq!(grade(any_loop, &labels(None, Some(Kind::Loop), None)), 1);
        assert_eq!(grade(any_loop, &labels(None, Some(Kind::OneShot), None)), 0);
        let am = FIXTURE_QUERIES
            .iter()
            .find(|s| s.key.is_some())
            .expect("key spec");
        let mut l = labels(Some(Category::Synth), Some(Kind::Loop), Some(100.0));
        l.key = Some(Key {
            root: 9,
            mode: Some(Mode::Minor),
        });
        assert_eq!(grade(am, &l), 2);
    }

    #[test]
    fn jsonl_parses_grades_and_reports_bad_lines() {
        let text = "# comment\n\
            {\"query\": \"dark kick\", \"relevant\": {\"k/a.wav\": 2, \"b.wav\": 1}}\n\
            {\"query\": \"x\", \"relevant\": {\"c.wav\": 9}}\n\
            not json\n\
            {\"query\": \" \", \"relevant\": {}}\n";
        let (qs, warnings) = parse_text_queries(text, Path::new("/g"));
        assert_eq!(qs.len(), 2);
        assert_eq!(qs[0].query, "dark kick");
        assert!(qs[0].relevant.contains(&(PathBuf::from("/g/k/a.wav"), 2)));
        assert!(qs[1].relevant.is_empty());
        assert_eq!(warnings.len(), 3, "{warnings:?}");
    }

    #[test]
    fn golden_file_is_optional_and_adds_unlabeled_files() {
        let tmp = tempfile::tempdir().expect("tmp");
        let mut ds = Dataset {
            tier: super::super::Tier::Golden,
            source: "t".into(),
            items: vec![],
            unlabeled: 0,
            missing: vec![],
            warnings: vec![],
            similar_queries: vec![],
            similar_extra: vec![],
            text_queries: vec![],
        };
        load_text_queries(&mut ds, tmp.path());
        assert!(ds.text_queries.is_empty() && ds.warnings.is_empty());
        std::fs::write(tmp.path().join("x.wav"), b"x").expect("write");
        std::fs::write(
            tmp.path().join(TEXT_QUERIES_FILE),
            "{\"query\": \"x\", \"relevant\": {\"x.wav\": 1, \"gone.wav\": 2}}\n",
        )
        .expect("write");
        load_text_queries(&mut ds, tmp.path());
        assert_eq!(ds.text_queries.len(), 1);
        assert_eq!(ds.similar_extra.len(), 1);
        assert_eq!(ds.similar_extra[0].id, "x.wav");
    }
}
