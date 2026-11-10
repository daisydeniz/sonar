//! Search behaviour on small in-RAM indexes.

use std::collections::BTreeSet;
use std::path::Path;

use super::*;
use crate::index::IndexDoc;

/// `(id, path below the library root "Lib")`.
const CORPUS: &[(i64, &str)] = &[
    (1, "Drums/Kicks/Kick_Hard_01.wav"),
    (2, "Drums/BD_Deep.wav"),
    (3, "Drums/Bass Drum Thump.wav"),
    (4, "Bass/Bass_Loop_128_Am.wav"),
    (5, "Bass/Sub_Cmin_120BPM.wav"),
    (6, "Bass/Growl C minor.wav"),
    (7, "Hats/HiHat_Closed.wav"),
    (8, "Hats/HH_Open_02.wav"),
    (9, "Loops/Vinyl Kick Loop 120 BPM.wav"),
    (10, "FX/Kickstart Riser.wav"),
    (11, "Keys/Rhodes One Shot C.wav"),
    (12, "Keys/Piano_OneShot_A.wav"),
    (13, "Pads/Cm Pad.wav"),
    (14, "Vox/Vocal_Am_Chop.wav"),
    (15, "Kicks/Thump_03.wav"),
    (16, "Keys/Rhodes Chord Loop.wav"),
];

fn corpus() -> TextIndex {
    let docs: Vec<IndexDoc> = CORPUS
        .iter()
        .map(|&(id, p)| IndexDoc::from_paths(id, Path::new("/v/Lib"), p))
        .collect();
    let ix = TextIndex::in_ram().expect("index");
    ix.upsert(&docs).expect("upsert");
    ix
}

fn run(ix: &TextIndex, q: &str, prefix: bool) -> SearchResults {
    let opts = SearchOptions { limit: 100, prefix };
    search(ix, &Synonyms::builtin(), q, &opts).expect("search")
}

/// Ranked ids, prefix matching on.
fn ranked(ix: &TextIndex, q: &str) -> Vec<i64> {
    run(ix, q, true).hits.iter().map(|h| h.file_id).collect()
}

/// Matching ids as a set, prefix matching off.
fn set(ix: &TextIndex, q: &str) -> BTreeSet<i64> {
    run(ix, q, false).hits.iter().map(|h| h.file_id).collect()
}

fn ids(v: &[i64]) -> BTreeSet<i64> {
    v.iter().copied().collect()
}

fn rank(list: &[i64], id: i64) -> usize {
    list.iter()
        .position(|&x| x == id)
        .unwrap_or_else(|| panic!("{id} not in {list:?}"))
}

#[test]
fn every_word_narrows() {
    let ix = corpus();
    assert_eq!(set(&ix, "bass loop"), ids(&[4]));
    assert_eq!(set(&ix, "rhodes loop"), ids(&[16]));
    assert_eq!(set(&ix, "rhodes"), ids(&[11, 16]));
    assert_eq!(set(&ix, "rhodes nothing"), ids(&[]));
}

#[test]
fn single_word_synonyms_work_both_ways() {
    let ix = corpus();
    let kicks = ids(&[1, 2, 3, 9, 15]);
    assert_eq!(set(&ix, "kick"), kicks);
    assert_eq!(set(&ix, "bd"), kicks);
    assert_eq!(set(&ix, "bassdrum"), kicks);
    assert_eq!(set(&ix, "hh"), ids(&[7, 8]));
    assert_eq!(set(&ix, "hihat"), ids(&[7, 8]));
    // Without synonyms only the literal spelling matches.
    let opts = SearchOptions {
        limit: 10,
        prefix: false,
    };
    let r = search(&ix, &Synonyms::empty(), "bd", &opts).expect("search");
    assert_eq!(r.hits.iter().map(|h| h.file_id).collect::<Vec<_>>(), [2]);
}

#[test]
fn multi_word_synonyms_work_both_ways() {
    let ix = corpus();
    // "bass drum" (phrase member) → the kick group, not every bass file.
    assert_eq!(set(&ix, "bass drum"), ids(&[1, 2, 3, 9, 15]));
    assert_eq!(set(&ix, "one shot"), ids(&[11, 12]));
    assert_eq!(set(&ix, "oneshot"), ids(&[11, 12]));
    assert_eq!(set(&ix, "rhodes oneshot"), ids(&[11]));
    // Order matters for phrases: "drum bass" is drum'n'bass, not the kick
    // group; only the file with both words matches.
    assert_eq!(set(&ix, "drum bass"), ids(&[3]));
}

#[test]
fn what_was_typed_outranks_synonyms_and_name_outranks_folder() {
    let ix = corpus();
    let r = ranked(&ix, "kick ");
    assert!(rank(&r, 1) < rank(&r, 2), "Kick above BD: {r:?}");
    assert!(
        rank(&r, 1) < rank(&r, 15),
        "name above Kicks/ folder: {r:?}"
    );
    let r = ranked(&ix, "kick");
    assert!(rank(&r, 1) < rank(&r, 10), "exact above prefix-only: {r:?}");
    assert!(
        rank(&r, 2) < rank(&r, 10),
        "synonym above prefix-only: {r:?}"
    );
}

#[test]
fn exclusions() {
    let ix = corpus();
    assert_eq!(set(&ix, "kick -vinyl"), ids(&[1, 2, 3, 15]));
    // Exclusions use synonyms too: -bd removes every kick.
    assert_eq!(set(&ix, "drums -bd"), ids(&[]));
    assert_eq!(set(&ix, "keys -\"one shot\""), ids(&[16]));
    assert_eq!(set(&ix, "keys -oneshot"), ids(&[16]));
    // Only exclusions: everything else.
    let all = run(&ix, "", false).total;
    assert_eq!(all, CORPUS.len());
    let r = run(&ix, "-kick -hh", false);
    assert_eq!(r.total, CORPUS.len() - 5 - 2);
    assert!(r
        .hits
        .iter()
        .all(|h| ![1, 2, 3, 9, 15, 7, 8].contains(&h.file_id)));
    // An exclusion that analyzes to nothing is ignored.
    assert_eq!(run(&ix, "kick -the", false).total, 5);
}

#[test]
fn prefix_as_you_type() {
    let ix = corpus();
    assert_eq!(ids(&ranked(&ix, "kic")), ids(&[1, 9, 10, 15]));
    assert!(set(&ix, "kic").is_empty(), "no prefix when disabled");
    assert!(ranked(&ix, "kic ").is_empty(), "finished word: no prefix");
    // Name prefix hits above folder-only prefix hits.
    let r = ranked(&ix, "kic");
    assert!(rank(&r, 1) < rank(&r, 15), "{r:?}");
    // Earlier words stay exact; only the last one is a prefix.
    assert_eq!(ids(&ranked(&ix, "vinyl ki")), ids(&[9]));
    assert_eq!(ids(&ranked(&ix, "rho")), ids(&[11, 16]));
    // `120b` is typed on the way to `120bpm`.
    assert_eq!(ids(&ranked(&ix, "120b")), ids(&[5, 9]));
    // A last word that may become a stop word (`bp` → `bpm`) or analyzes
    // to nothing does not empty the list while typing.
    assert_eq!(ids(&ranked(&ix, "120 bp")), ids(&[5, 9]));
    assert_eq!(ids(&ranked(&ix, "rhodes th")), ids(&[11, 16]));
    assert_eq!(ids(&ranked(&ix, "rhodes 01")), ids(&[11, 16]));
}

#[test]
fn bpm_and_key_queries() {
    let ix = corpus();
    for q in ["120 bpm", "120bpm", "120BPM", "bpm 120", "120"] {
        assert_eq!(set(&ix, q), ids(&[5, 9]), "{q}");
    }
    for q in ["Cmin", "cmin", "c minor", "Cm", "cm", "C minor"] {
        assert_eq!(set(&ix, q), ids(&[5, 6, 13]), "{q}");
    }
    for q in ["am", "Am", "a minor", "Amin"] {
        assert_eq!(set(&ix, q), ids(&[4, 14]), "{q}");
    }
    // A minor is not A major / bare A.
    assert!(!set(&ix, "am").contains(&12));
    assert_eq!(set(&ix, "bass am"), ids(&[4]));
}

#[test]
fn plurals_fold_both_ways() {
    let ix = corpus();
    assert_eq!(set(&ix, "kicks"), set(&ix, "kick"));
    assert_eq!(set(&ix, "loops"), ids(&[4, 9, 16]));
    assert_eq!(set(&ix, "pad"), ids(&[13]));
}

#[test]
fn limit_caps_hits_not_total() {
    let ix = corpus();
    let opts = SearchOptions {
        limit: 2,
        prefix: false,
    };
    let r = search(&ix, &Synonyms::builtin(), "kick", &opts).expect("search");
    assert_eq!((r.hits.len(), r.total), (2, 5));
    let opts = SearchOptions { limit: 0, ..opts };
    let r = search(&ix, &Synonyms::builtin(), "kick", &opts).expect("search");
    assert_eq!((r.hits.len(), r.total), (0, 5));
    assert!(r.hits.is_empty());
}

#[test]
fn odd_queries_do_not_fail() {
    let ix = corpus();
    for q in [
        "",
        " ",
        "-",
        "\"",
        "\"\"",
        "--",
        "(",
        "a|b",
        "c#",
        "key:",
        "*",
        ".*",
        "\\",
        "ü",
        "🥁 kick",
        "-\"",
        "kick -",
    ] {
        assert!(
            search(&ix, &Synonyms::builtin(), q, &SearchOptions::default()).is_ok(),
            "{q:?}"
        );
    }
}
