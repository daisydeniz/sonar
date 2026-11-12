//! Hybrid search on a small in-RAM index with a stand-in text encoder.

use std::path::Path;

use super::*;
use crate::index::IndexDoc;
use crate::search::semantic::tests::unit;
use crate::search::{Filters, Sort};

/// `(id, path below "Lib", embedding axis, tilt, hidden)`: kicks sit on
/// axis 0, hats on axis 1, the rest on axis 3.
const CORPUS: &[(i64, &str, usize, f32, bool)] = &[
    (1, "Drums/Kick_Hard_01.wav", 0, 0.05, false),
    (2, "Drums/BD_Deep.wav", 0, 0.10, false),
    (3, "Drums/Thump.wav", 0, 0.40, false),
    (4, "Hats/HiHat_Closed.wav", 1, 0.00, false),
    (5, "Hats/HH_Open_02.wav", 1, 0.20, false),
    (6, "Loops/Vinyl Kick Loop.wav", 0, 0.00, false),
    (7, "Pads/Warm Pad.wav", 3, 0.00, false),
    (8, "Kicks/Thump_03.wav", 0, 0.80, false),
    (9, "Drums/Secret_Kick.wav", 0, 0.01, true),
];

fn corpus() -> (TextIndex, SemanticIndex) {
    let docs: Vec<IndexDoc> = CORPUS
        .iter()
        .map(|&(id, p, axis, _, hidden)| IndexDoc {
            category: Some(if axis == 0 { "kick" } else { "other" }.into()),
            hidden,
            ..IndexDoc::from_paths(id, Path::new("/v/Lib"), p)
        })
        .collect();
    let ix = TextIndex::in_ram().expect("index");
    ix.upsert(&docs).expect("upsert");
    let sem = SemanticIndex::from_embeddings(
        CORPUS
            .iter()
            .map(|&(id, _, axis, tilt, _)| (id, unit(axis, tilt))),
    );
    (ix, sem)
}

/// Maps words to axes: anything kick-like → 0, hats → 1, else axis 5.
struct Words;

impl QueryEmbedder for Words {
    fn embed_query(&mut self, text: &str) -> Result<Embedding> {
        let t = text.to_lowercase();
        let axis = if ["kick", "thump", "boom"].iter().any(|w| t.contains(w)) {
            0
        } else if t.contains("hat") {
            1
        } else {
            5
        };
        Ok(unit(axis, 0.0))
    }
}

struct Broken;

impl QueryEmbedder for Broken {
    fn embed_query(&mut self, _: &str) -> Result<Embedding> {
        Err(Error::Embed {
            reason: "boom".into(),
        })
    }
}

fn run(ix: &TextIndex, sem: Option<&SemanticSearch>, q: &str, mode: SearchMode) -> HybridResults {
    run_with(ix, sem, q, mode, SearchOptions::default())
}

fn run_with(
    ix: &TextIndex,
    sem: Option<&SemanticSearch>,
    q: &str,
    mode: SearchMode,
    search: SearchOptions,
) -> HybridResults {
    let opts = HybridOptions {
        mode,
        search,
        cancel: None,
    };
    hybrid_search(ix, &Synonyms::builtin(), sem, q, &opts).expect("hybrid")
}

fn ids(r: &HybridResults) -> Vec<i64> {
    r.hits.iter().map(|h| h.file_id).collect()
}

fn engine() -> (TextIndex, SemanticSearch) {
    let (ix, idx) = corpus();
    (ix, SemanticSearch::new(Box::new(Words), idx))
}

#[test]
fn hybrid_ranks_files_found_by_both_legs_first() {
    let (ix, sem) = engine();
    let r = run(&ix, Some(&sem), "thump", SearchMode::Hybrid);
    assert_eq!((r.mode, r.fallback.as_deref()), (SearchMode::Hybrid, None));
    // BM25 finds 3 and 8 by name; CLAP ranks every kick: 6, 1, 2, 3, 8
    // (9 is hidden). Both legs agree on 3 and 8, so they lead.
    let got = ids(&r);
    let mut top2 = got[..2].to_vec();
    top2.sort_unstable();
    assert_eq!(top2, [3, 8], "{got:?}");
    assert_eq!(&got[2..5], &[6, 1, 2], "{got:?}");
    assert!(!got.contains(&9), "hidden file returned: {got:?}");
    assert_eq!(r.text_total, 2);
    assert_eq!(r.semantic_candidates, 8);
    let six = r.hits.iter().find(|h| h.file_id == 6).expect("6");
    assert_eq!((six.text_rank, six.semantic_rank), (None, Some(1)));
    assert!(r.hits.windows(2).all(|w| w[0].score >= w[1].score));
}

#[test]
fn natural_language_finds_files_bm25_cannot() {
    let (ix, sem) = engine();
    let text = run(&ix, Some(&sem), "booming low end", SearchMode::Text);
    assert!(text.hits.is_empty());
    let r = run(&ix, Some(&sem), "booming low end", SearchMode::Hybrid);
    assert_eq!(&ids(&r)[..3], &[6, 1, 2]);
    let s = run(&ix, Some(&sem), "booming low end", SearchMode::Semantic);
    assert_eq!(s.mode, SearchMode::Semantic);
    assert_eq!(&ids(&s)[..3], &[6, 1, 2]);
    assert!(s.hits[0].score > 0.99 && s.facets.is_none());
}

#[test]
fn exclusions_and_filters_restrict_both_legs() {
    let (ix, sem) = engine();
    for mode in [SearchMode::Hybrid, SearchMode::Semantic] {
        let r = run(&ix, Some(&sem), "kick -vinyl", mode);
        assert!(!ids(&r).contains(&6), "{mode:?}: {:?}", ids(&r));
        assert!(ids(&r).contains(&1));
        // Synonyms apply to exclusions too: -bd excludes "BD_Deep".
        let r = run(&ix, Some(&sem), "thump -bd", mode);
        assert!(!ids(&r).contains(&2), "{mode:?}");
    }
    let only_kicks = SearchOptions {
        filters: Filters {
            categories: vec!["kick".into()],
            ..Filters::default()
        },
        ..SearchOptions::default()
    };
    let r = run_with(
        &ix,
        Some(&sem),
        "hihat",
        SearchMode::Hybrid,
        only_kicks.clone(),
    );
    let got = ids(&r);
    assert!(
        !got.is_empty() && got.iter().all(|id| [1, 2, 3, 6, 8].contains(id)),
        "{got:?}"
    );
    let mut hidden_too = only_kicks;
    hidden_too.filters.include_hidden = true;
    let r = run_with(&ix, Some(&sem), "kick", SearchMode::Semantic, hidden_too);
    assert_eq!(ids(&r)[0], 6);
    assert!(ids(&r).contains(&9));
}

#[test]
fn falls_back_to_text_without_error() {
    let (ix, sem) = engine();
    let plain = search(
        &ix,
        &Synonyms::builtin(),
        "thump",
        &SearchOptions::default(),
    )
    .expect("search");
    let plain_ids: Vec<i64> = plain.hits.iter().map(|h| h.file_id).collect();
    // No semantic resources.
    let r = run(&ix, None, "thump", SearchMode::Hybrid);
    assert_eq!((r.mode, ids(&r)), (SearchMode::Text, plain_ids.clone()));
    assert!(r.fallback.is_some());
    // An index without embeddings.
    let empty = SemanticSearch::new(Box::new(Words), SemanticIndex::from_embeddings([]));
    let r = run(&ix, Some(&empty), "thump", SearchMode::Semantic);
    assert_eq!((r.mode, ids(&r)), (SearchMode::Text, plain_ids.clone()));
    // A failing encoder.
    let (ix2, idx) = corpus();
    let broken = SemanticSearch::new(Box::new(Broken), idx);
    for mode in [SearchMode::Hybrid, SearchMode::Semantic] {
        let r = run(&ix2, Some(&broken), "thump", mode);
        assert_eq!((r.mode, ids(&r)), (SearchMode::Text, plain_ids.clone()));
        assert!(r.fallback.as_deref().is_some_and(|f| f.contains("encoder")));
    }
    // Text mode is plain search; browse and field sorts stay text.
    let r = run(&ix, Some(&sem), "thump", SearchMode::Text);
    assert_eq!(
        (r.mode, ids(&r), r.fallback),
        (SearchMode::Text, plain_ids, None)
    );
    let browse = run(&ix, Some(&sem), "-vinyl", SearchMode::Hybrid);
    assert_eq!(browse.mode, SearchMode::Text);
    assert_eq!(browse.hits.len(), 7);
    let by_name = SearchOptions {
        sort: Sort::by(SortField::Name),
        ..SearchOptions::default()
    };
    let r = run_with(&ix, Some(&sem), "kick", SearchMode::Hybrid, by_name);
    assert_eq!(r.mode, SearchMode::Text);
}

#[test]
fn hybrid_keeps_the_limit_and_text_facets() {
    let (ix, sem) = engine();
    let opts = SearchOptions {
        limit: 2,
        facets: true,
        ..SearchOptions::default()
    };
    let r = run_with(&ix, Some(&sem), "thump", SearchMode::Hybrid, opts);
    assert_eq!(r.hits.len(), 2);
    let f = r.facets.expect("facets");
    assert_eq!(f.categories.iter().map(|c| c.count).sum::<usize>(), 2);
}

#[test]
fn cancelled_queries_stop() {
    let (ix, sem) = engine();
    let opts = HybridOptions {
        mode: SearchMode::Hybrid,
        search: SearchOptions::default(),
        cancel: Some(Arc::new(AtomicBool::new(true))),
    };
    let r = hybrid_search(&ix, &Synonyms::builtin(), Some(&sem), "kick", &opts);
    assert!(matches!(r, Err(Error::Cancelled)));
}

#[test]
fn rrf_sums_reciprocal_ranks() {
    let hit = |file_id| SemanticHit {
        file_id,
        cosine: 0.5,
    };
    let fused = rrf(&[10, 20, 30], &[hit(30), hit(40), hit(10)]);
    let got: Vec<(i64, Option<u32>, Option<u32>)> = fused
        .iter()
        .map(|h| (h.file_id, h.text_rank, h.semantic_rank))
        .collect();
    // 10: 1/61 + 1/63; 30: 1/63 + 1/61 (tie: best rank 1 each → id);
    // 20: 1/62; 40: 1/62 (tie: best rank 2 each → id).
    assert_eq!(
        got,
        [
            (10, Some(1), Some(3)),
            (30, Some(3), Some(1)),
            (20, Some(2), None),
            (40, None, Some(2)),
        ]
    );
    let want = 1.0 / 61.0 + 1.0 / 63.0;
    assert!((f64::from(fused[0].score) - want).abs() < 1e-6);
    assert!(rrf(&[], &[]).is_empty());
}

#[test]
fn modes_parse_and_semantic_text_drops_exclusions() {
    assert_eq!("Hybrid".parse(), Ok(SearchMode::Hybrid));
    assert_eq!(" text".parse(), Ok(SearchMode::Text));
    assert_eq!("semantic".parse(), Ok(SearchMode::Semantic));
    assert!("bm25".parse::<SearchMode>().is_err());
    assert_eq!(SearchMode::default().as_str(), "hybrid");
    assert_eq!(
        semantic_text("dark  kick -vinyl \"one shot\" -\"lo fi\""),
        "dark kick one shot"
    );
    assert_eq!(semantic_text(" -vinyl "), "");
}
