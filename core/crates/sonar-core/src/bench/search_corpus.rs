//! Synthetic sample-library rows for the search benchmark: deterministic
//! (seeded) file, analysis, metadata and user-data rows written straight
//! into a [`Store`] (the scanner and analysis are bypassed; the text index
//! is then built from the store by the production path).
//!
//! Layout: `<library>/<Pack Name N>/<group folder>/<name>.wav`, 800 files
//! per pack in 16 folders of 50 (one category per folder, every category
//! once per pack). Names follow common sample-pack styles: pack prefixes
//! (`TE_Kick_Punchy_12`), spaces, camelCase (`darkPad03`), dashes, and for
//! loops BPM / key / `Loop` suffixes. Files are spread over five libraries.
//!
//! Effective values (ADR-018) cover the usual gaps: some categories,
//! kinds, BPMs and keys are unknown to the analysis, some loops carry the
//! BPM only in the name (filename prior), a few files have a category
//! override, tags, favourite / hidden flags, or are `missing` (not
//! indexed).

use std::fs;
use std::path::Path;

use rusqlite::params;

use crate::search::keys::{key_label, KEY_CODES};
use crate::{Error, Result, Store};

/// Files per leaf folder.
const PER_LEAF: usize = 50;
/// Files per pack (one leaf per category).
pub(crate) const PER_PACK: usize = PER_LEAF * CATS.len();

/// Library folder names; packs go to them in the ratio of [`LIBRARY_OF_PACK`].
pub const LIBRARY_NAMES: [&str; 5] = [
    "Splice",
    "Loopmasters",
    "Vengeance",
    "Cymatics",
    "My Recordings",
];
/// Library of pack `n % 10`: ratio 4:2:2:1:1, every library within the
/// first five packs.
const LIBRARY_OF_PACK: [usize; 10] = [0, 1, 2, 3, 4, 0, 1, 2, 0, 0];

/// Pack names and the prefixes their files carry.
const PACKS: [(&str, &str); 16] = [
    ("Trap Essentials", "TE"),
    ("Deep House Vol", "DHV"),
    ("Lofi Dreams", "LD"),
    ("Techno Warehouse", "TW"),
    ("Future Bass", "FB"),
    ("Drill Kit", "DK"),
    ("Afro House", "AH"),
    ("Ambient Textures", "AT"),
    ("Boom Bap Vault", "BBV"),
    ("Neo Soul Keys", "NSK"),
    ("Hyperpop", "HP"),
    ("UK Garage", "UKG"),
    ("Cinematic FX", "CFX"),
    ("Dubstep Arsenal", "DA"),
    ("Jungle Breaks", "JB"),
    ("Pop Vocals", "PV"),
];

struct Cat {
    category: &'static str,
    folder: &'static str,
    words: &'static [&'static str],
    /// Share of loops, per mille.
    loops: u64,
    /// Has a musical key.
    tonal: bool,
}

const fn cat(
    category: &'static str,
    folder: &'static str,
    words: &'static [&'static str],
    loops: u64,
    tonal: bool,
) -> Cat {
    Cat {
        category,
        folder,
        words,
        loops,
        tonal,
    }
}

#[rustfmt::skip]
const CATS: [Cat; 16] = [
    cat("kick", "Drums/Kicks", &["Kick", "BD", "Kick", "Bassdrum", "Kik"], 30, false),
    cat("snare", "Drums/Snares", &["Snare", "SD", "Snare", "Rim", "Snr"], 30, false),
    cat("hat", "Drums/Hats", &["HH", "Hat", "HiHat", "OpenHat", "Closed_Hat"], 150, false),
    cat("clap", "Drums/Claps", &["Clap", "Clp", "Handclap", "Snap"], 20, false),
    cat("perc", "Drums/Percussion", &["Perc", "Shaker", "Tamb", "Conga", "Cowbell"], 250, false),
    cat("cymbal", "Drums/Cymbals", &["Crash", "Ride", "Cymbal", "Cym"], 20, false),
    cat("drums", "Loops/Drums", &["Drum_Loop", "Top_Loop", "Breakbeat", "Groove", "Drums"], 950, false),
    cat("bass", "Bass/808", &["808", "Sub", "Bass", "Reese", "808_Glide"], 400, true),
    cat("pad", "Synths/Pads", &["Pad", "Atmo", "Drone", "Texture"], 500, true),
    cat("lead", "Synths/Leads", &["Lead", "Synth", "Arp", "Pluck"], 600, true),
    cat("keys", "Keys", &["Piano", "EPiano", "Rhodes", "Chord", "Keys"], 700, true),
    cat("vocal", "Vocals", &["Vox", "Vocal", "Vocal_Chop", "Acapella", "Phrase"], 600, true),
    cat("fx", "FX", &["FX", "Riser", "Impact", "Whoosh", "Sweep", "Reverse"], 100, false),
    cat("guitar", "Loops/Guitar", &["Gtr", "Guitar", "Strum", "Riff"], 900, true),
    cat("strings", "Orchestral/Strings", &["Strings", "Violin", "Cello", "Orch"], 700, true),
    cat("brass", "Orchestral/Brass", &["Brass", "Horn", "Trumpet", "Stab"], 500, true),
];

#[rustfmt::skip]
const ADJ: [&str; 34] = [
    "Dark", "Bright", "Punchy", "Deep", "Airy", "Warm", "Dusty", "Lofi", "Crispy", "Vintage",
    "Analog", "Hard", "Soft", "Tight", "Wide", "Glide", "Gritty", "Clean", "Distorted", "Tape",
    "Vinyl", "Heavy", "Short", "Long", "Fat", "Metallic", "Organic", "Ambient", "Trap", "House",
    "Techno", "Open", "Closed", "Dreamy",
];

/// User tags given to a few files.
pub const TAGS: [&str; 6] = [
    "mix-ready",
    "favourite kit",
    "todo",
    "client-x",
    "vocal ideas",
    "dark",
];

/// What [`generate_store`] wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchCorpus {
    /// File rows written (including `missing` ones).
    pub files: usize,
    /// Rows with `status = 'missing'` (never indexed).
    pub missing: usize,
    /// `libraries.id` of each entry of [`LIBRARY_NAMES`].
    pub libraries: Vec<i64>,
}

/// Small deterministic PRNG (SplitMix64, Steele et al., OOPSLA 2014).
pub(crate) struct Rng(u64);

impl Rng {
    pub(crate) fn new(seed: u64, i: usize) -> Self {
        Self(seed ^ (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15))
    }

    pub(crate) fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }

    fn chance(&mut self, per_mille: u64) -> bool {
        self.next() % 1000 < per_mille
    }

    fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[self.below(xs.len())]
    }
}

/// One generated file with its analysis and user data.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FileRow {
    pub library: usize,
    pub rel_path: String,
    pub category: Option<&'static str>,
    pub kind: Option<&'static str>,
    pub bpm: Option<f64>,
    pub key: Option<String>,
    pub duration_ms: u64,
    pub tags: Vec<usize>,
    pub favorite: bool,
    pub hidden: bool,
    pub missing: bool,
    pub category_override: Option<&'static str>,
}

/// Number of folder categories (see [`pack_and_family`]).
pub(crate) const FAMILIES: usize = CATS.len();

/// Pack number and folder category (index into the category table; the
/// truth, before the analysis "unknown" gaps) of file number `i`.
pub(crate) fn pack_and_family(i: usize) -> (usize, usize) {
    let pack = i / PER_PACK;
    let leaf = (i % PER_PACK) / PER_LEAF;
    // `pack * 5 + leaf` (mod 16) runs over every category once per pack.
    (pack, (pack * 5 + leaf) % CATS.len())
}

/// File number `i` of the corpus with `seed`; depends on nothing else.
pub(crate) fn file_row(seed: u64, i: usize) -> FileRow {
    let mut r = Rng::new(seed, i);
    let (pack, family) = pack_and_family(i);
    let c = &CATS[family];
    let (pack_name, abbr) = PACKS[pack % PACKS.len()];
    let nn = i % PER_LEAF + 1;
    let word = *r.pick(c.words);
    let adj = *r.pick(&ADJ);
    let is_loop = r.chance(c.loops);
    let bpm = if is_loop || r.chance(50) {
        let whole = 70 + r.below(106) as u32;
        Some(f64::from(whole) + if r.chance(100) { 0.5 } else { 0.0 })
    } else {
        None
    };
    let key = (c.tonal && r.chance(850)).then(|| key_label(r.below(usize::from(KEY_CODES)) as u8));

    let mut name = match r.below(5) {
        0 => format!("{abbr}_{word}_{adj}_{nn:02}"),
        1 => format!("{word} {adj} {nn:02}"),
        2 => format!("{}{word}{nn:02}", adj.to_lowercase()),
        3 => format!("{}-{}-{nn:02}", word.to_lowercase(), adj.to_lowercase()),
        _ => format!("{abbr} {adj} {word} {nn:02}"),
    };
    // BPM / key in the name: the filename prior fills in what analysis lacks.
    let bpm_in_name = bpm.filter(|_| is_loop && r.chance(800));
    if let Some(b) = bpm_in_name {
        let b = b.floor();
        name.push_str(&match r.below(3) {
            0 => format!("_{b}bpm"),
            1 => format!(" {b} BPM"),
            _ => format!("_{b}"),
        });
    }
    if let Some(k) = key.as_deref().filter(|_| r.chance(600)) {
        name.push('_');
        name.push_str(k);
    }
    if is_loop && r.chance(500) {
        name.push_str("_Loop");
    }

    let duration_ms = match bpm.filter(|_| is_loop) {
        // 4–32 beats.
        Some(b) => (60_000.0 / b * f64::from(4u32 << r.below(4))) as u64,
        None => 60 + r.below(4_000) as u64,
    };
    let tags = if r.chance(30) {
        let n = 1 + r.below(2);
        (0..n).map(|_| r.below(TAGS.len())).collect()
    } else {
        Vec::new()
    };
    FileRow {
        library: LIBRARY_OF_PACK[pack % LIBRARY_OF_PACK.len()],
        rel_path: format!(
            "{pack_name} {}/{}/{name}.wav",
            pack / PACKS.len() + 1,
            c.folder
        ),
        category: r.chance(850).then_some(c.category),
        kind: r
            .chance(900)
            .then_some(if is_loop { "loop" } else { "one_shot" }),
        // Some loops have the BPM only in the name.
        bpm: bpm.filter(|_| bpm_in_name.is_none() || r.chance(700)),
        key: key.filter(|_| r.chance(800)),
        duration_ms,
        tags,
        favorite: r.chance(20),
        hidden: r.chance(10),
        missing: r.chance(5),
        category_override: r.chance(3).then(|| r.pick(&CATS).category),
    }
}

/// Creates the library folders under `root` (empty: only the store rows
/// refer to files) and writes `files` file rows with analysis, metadata
/// and user data into `store`, in one transaction.
///
/// # Errors
/// [`Error::Io`] (library folders), [`Error::Sqlite`].
pub fn generate_store(store: &Store, root: &Path, files: usize, seed: u64) -> Result<SearchCorpus> {
    let mut libraries = Vec::new();
    for name in LIBRARY_NAMES {
        let dir = root.join(name);
        fs::create_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
        libraries.push(store.add_library(&dir)?.id);
    }
    let tx = store.conn().unchecked_transaction()?;
    for t in TAGS {
        tx.execute("INSERT OR IGNORE INTO tags (name) VALUES (?1)", [t])?;
    }
    let mut tag_ids = Vec::new();
    for t in TAGS {
        tag_ids.push(
            tx.query_row("SELECT id FROM tags WHERE name = ?1", [t], |r| {
                r.get::<_, i64>(0)
            })?,
        );
    }
    let mut missing = 0;
    {
        let mut file = tx.prepare(
            "INSERT INTO files (id, library_id, rel_path, size, mtime, status, analysis_version) \
             VALUES (?1, ?2, ?3, ?4, 0, ?5, 1)",
        )?;
        let mut analysis = tx.prepare(
            "INSERT INTO analysis (file_id, kind, bpm, bpm_source, key, key_source, category, category_source) \
             VALUES (?1, ?2, ?3, 'detected', ?4, 'detected', ?5, 'detected')",
        )?;
        let mut meta = tx.prepare(
            "INSERT INTO audio_meta (file_id, duration_ms, sample_rate, channels, bit_depth) \
             VALUES (?1, ?2, 44100, 2, 24)",
        )?;
        let mut tag =
            tx.prepare("INSERT OR IGNORE INTO file_tags (file_id, tag_id) VALUES (?1, ?2)")?;
        let mut fav = tx.prepare("INSERT INTO favorites (file_id, added_at) VALUES (?1, 0)")?;
        let mut hide = tx.prepare("INSERT INTO hidden (file_id) VALUES (?1)")?;
        let mut over = tx
            .prepare("INSERT INTO overrides (file_id, field, value) VALUES (?1, 'category', ?2)")?;
        for i in 0..files {
            let row = file_row(seed, i);
            let id = i as i64 + 1;
            let status = if row.missing { "missing" } else { "analyzed" };
            missing += usize::from(row.missing);
            let size = row.duration_ms * 264 + 44;
            file.execute(params![
                id,
                libraries[row.library],
                row.rel_path,
                size as i64,
                status
            ])?;
            analysis.execute(params![id, row.kind, row.bpm, row.key, row.category])?;
            meta.execute(params![id, row.duration_ms as i64])?;
            for &t in &row.tags {
                tag.execute(params![id, tag_ids[t]])?;
            }
            if row.favorite {
                fav.execute([id])?;
            }
            if row.hidden {
                hide.execute([id])?;
            }
            if let Some(c) = row.category_override {
                over.execute(params![id, c])?;
            }
        }
    }
    tx.commit()?;
    Ok(SearchCorpus {
        files,
        missing,
        libraries,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use crate::search::keys::parse_key;

    #[test]
    fn rows_are_deterministic_unique_and_varied() {
        let mut paths = HashSet::new();
        let (mut loops, mut keys, mut bpms, mut libs) = (0, 0, 0, HashSet::new());
        for i in 0..4_000 {
            let r = file_row(7, i);
            assert_eq!(r, file_row(7, i));
            assert!(
                paths.insert((r.library, r.rel_path.clone())),
                "{}",
                r.rel_path
            );
            loops += usize::from(r.kind == Some("loop"));
            bpms += usize::from(r.bpm.is_some());
            if let Some(k) = &r.key {
                assert!(parse_key(k).is_some(), "{k}");
                keys += 1;
            }
            libs.insert(r.library);
        }
        assert_ne!(file_row(7, 1), file_row(8, 1));
        assert!(
            loops > 500 && keys > 500 && bpms > 500,
            "{loops} {keys} {bpms}"
        );
        assert_eq!(libs.len(), LIBRARY_NAMES.len());
    }

    #[test]
    fn writes_rows_into_the_store() {
        let tmp = tempfile::tempdir().expect("tmp");
        let store = Store::open(tmp.path().join("s.db")).expect("store");
        let c = generate_store(&store, &tmp.path().join("libs"), 1_000, 1).expect("generate");
        assert_eq!(c.libraries.len(), LIBRARY_NAMES.len());
        let n: i64 = store
            .conn()
            .query_row("SELECT count(*) FROM files", [], |r| r.get(0))
            .expect("count");
        assert_eq!(n, 1_000);
        assert!(c.missing < 30);
    }
}
