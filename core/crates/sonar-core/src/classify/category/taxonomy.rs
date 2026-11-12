//! The category taxonomy (`docs/categories.md` mirrors this table; a test
//! checks that every id, prompt and keyword below appears there).
//!
//! A flat set of [`Category`] ids. Category is the *sound source*, kind
//! (loop / one-shot) is separate: a kick loop is `kick`, a full drum loop
//! or fill is `drums`. Per category:
//! - **prompts**: CLAP zero-shot text prompts (Wu et al. 2023), one per
//!   typical sub-type of the class (`a shaker`, `a conga drum`, ...); the
//!   class score is the best-matching prompt ([`super::clap`]). Selected on
//!   the training half of the silver set (ADR-028);
//! - **words**: file/folder name tokens (as the ADR-016 tokenizer folds
//!   them, plurals folded) that name the category;
//! - **weak words**: generic words that name it only when no other
//!   category word is in the same name segment (`Snare Fills` is snare,
//!   `Fills` is drums).

use serde::{Deserialize, Serialize};

/// A sample category (see the module docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    /// Kick / bass drum.
    Kick,
    /// Snare drum (also rolls, snare fills).
    Snare,
    /// Hand clap, finger snap.
    Clap,
    /// Hi-hat, open or closed.
    Hat,
    /// Crash, ride, splash, china.
    Cymbal,
    /// Tom-tom.
    Tom,
    /// Other percussion: shaker, conga, bongo, tambourine, rim, cowbell, ...
    Perc,
    /// Several drums together: drum loops, top loops, fills, breaks.
    Drums,
    /// Bass, sub, 808 bass.
    Bass,
    /// Lead synth / melody.
    Lead,
    /// Pad.
    Pad,
    /// Piano, electric piano, organ.
    Keys,
    /// Pluck.
    Pluck,
    /// Other synth parts: chords, stabs, arps.
    Synth,
    /// Voice: vocals, vox, chants.
    Vocal,
    /// Sound effects: risers, downlifters, impacts, whooshes.
    Fx,
    /// Atmospheres, drones, ambience, foley.
    Texture,
}

/// One row of the taxonomy table.
#[derive(Debug, Clone, Copy)]
pub struct CategoryInfo {
    /// The category.
    pub category: Category,
    /// Stable id (stored in `analysis.category`, used in filters).
    pub id: &'static str,
    /// CLAP prompts (class score = best prompt).
    pub prompts: &'static [&'static str],
    /// Name words (folded, singular); a space joins consecutive tokens.
    pub words: &'static [&'static str],
    /// Generic name words (see the module docs).
    pub weak_words: &'static [&'static str],
}

/// Number of categories.
pub const N_CATEGORIES: usize = 17;

/// The taxonomy, in [`Category`] order.
pub const TAXONOMY: [CategoryInfo; N_CATEGORIES] = [
    CategoryInfo {
        category: Category::Kick,
        id: "kick",
        prompts: &["a kick drum", "an electronic kick drum"],
        words: &["kick", "kik", "bd", "bassdrum", "bass drum"],
        weak_words: &[],
    },
    CategoryInfo {
        category: Category::Snare,
        id: "snare",
        prompts: &["a snappy snare", "a snare roll"],
        words: &["snare", "sd", "snr"],
        weak_words: &[],
    },
    CategoryInfo {
        category: Category::Clap,
        id: "clap",
        prompts: &[
            "a hand clap",
            "the sound of a clap",
            "a finger snap",
            "a clap",
            "an electronic clap",
            "people clapping",
            "a layered handclap with reverb",
            "a drum machine clap",
        ],
        words: &["clap", "clp", "handclap", "snap", "fingersnap"],
        weak_words: &[],
    },
    CategoryInfo {
        category: Category::Hat,
        id: "hat",
        prompts: &[
            "a hi-hat",
            "the sound of a closed hi-hat",
            "an open hi-hat",
            "a closed hi-hat",
            "a hi-hat loop",
            "hi-hats",
        ],
        words: &["hat", "hihat", "hh", "ohh", "chh"],
        weak_words: &[],
    },
    CategoryInfo {
        category: Category::Cymbal,
        id: "cymbal",
        prompts: &[
            "a crash cymbal",
            "a ride cymbal",
            "the sound of a cymbal",
            "a cymbal crash",
            "a reverse cymbal",
            "a ride cymbal bell",
        ],
        words: &["cymbal", "cym", "crash", "ride", "splash", "china"],
        weak_words: &[],
    },
    CategoryInfo {
        category: Category::Tom,
        id: "tom",
        prompts: &["the sound of a tom-tom drum hit"],
        words: &["tom", "tomtom"],
        weak_words: &[],
    },
    CategoryInfo {
        category: Category::Perc,
        id: "perc",
        prompts: &[
            "a percussion hit",
            "a shaker",
            "a conga drum",
            "a tambourine",
            "a bongo",
            "a cowbell",
            "a rimshot",
            "a woodblock",
            "a percussion loop",
            "a click",
            "a short metallic percussion hit",
        ],
        words: &[
            "perc",
            "percussion",
            "percusion",
            "shaker",
            "conga",
            "bongo",
            "tambourine",
            "tamb",
            "cowbell",
            "clave",
            "woodblock",
            "timbale",
            "guiro",
            "cabasa",
            "rim",
            "rimshot",
        ],
        weak_words: &[],
    },
    CategoryInfo {
        category: Category::Drums,
        id: "drums",
        prompts: &[
            "a drum loop",
            "a drum beat",
            "a drum fill",
            "a breakbeat",
            "drums playing a groove",
            "a drum kit pattern",
        ],
        words: &["drumloop", "breakbeat"],
        weak_words: &["drum", "top", "fill", "break", "kit"],
    },
    CategoryInfo {
        category: Category::Bass,
        id: "bass",
        prompts: &[
            "a bass synth",
            "a bass line",
            "an 808 bass",
            "a sub bass",
            "a deep bass note",
            "a distorted bass",
            "an electric bass guitar",
        ],
        words: &["bass", "bassline", "subbass", "reese"],
        weak_words: &["808"],
    },
    CategoryInfo {
        category: Category::Lead,
        id: "lead",
        prompts: &["a synth lead melody", "a lead synthesizer", "a lead melody"],
        words: &["lead"],
        weak_words: &[],
    },
    CategoryInfo {
        category: Category::Pad,
        id: "pad",
        prompts: &[
            "a synth pad",
            "a warm sustained pad chord",
            "an ambient pad",
            "a string pad",
        ],
        words: &["pad"],
        weak_words: &[],
    },
    CategoryInfo {
        category: Category::Keys,
        id: "keys",
        prompts: &[
            "a piano",
            "an electric piano",
            "an organ",
            "piano chords",
            "a piano note",
            "a rhodes",
        ],
        words: &[
            "keys",
            "piano",
            "epiano",
            "rhodes",
            "organ",
            "wurli",
            "wurlitzer",
            "clav",
            "clavinet",
            "keyboard",
        ],
        weak_words: &[],
    },
    CategoryInfo {
        category: Category::Pluck,
        id: "pluck",
        prompts: &["a plucked synth", "a plucked string"],
        words: &["pluck", "plucked"],
        weak_words: &[],
    },
    CategoryInfo {
        category: Category::Synth,
        id: "synth",
        prompts: &[
            "a synthesizer",
            "synth chords",
            "a synth arpeggio",
            "a synth stab",
            "an acid synth",
            "a synth chord stab",
            "a synth hit",
            "an electronic synth sound",
        ],
        words: &["synth", "syn", "stab"],
        weak_words: &["chord", "arp", "arpeggio"],
    },
    CategoryInfo {
        category: Category::Vocal,
        id: "vocal",
        prompts: &[
            "a human voice",
            "a vocal sample",
            "singing",
            "a vocal chop",
            "a person speaking",
        ],
        words: &[
            "vocal", "vox", "voc", "voice", "acapella", "chant", "choir", "adlib", "spoken",
        ],
        weak_words: &[],
    },
    CategoryInfo {
        category: Category::Fx,
        id: "fx",
        prompts: &[
            "a sound effect",
            "a riser sweep",
            "an impact sound effect",
            "a whoosh",
            "a white noise sweep",
            "a downlifter",
            "a sci-fi zap",
            "a boom impact",
            "a rising noise sweep",
            "a transition effect",
        ],
        words: &[
            "fx",
            "sfx",
            "effect",
            "riser",
            "uplifter",
            "downlifter",
            "impact",
            "whoosh",
            "swoosh",
            "sweep",
            "buildup",
            "boom",
            "zap",
            "laser",
            "transition",
        ],
        weak_words: &["reverse", "noise", "drop"],
    },
    CategoryInfo {
        category: Category::Texture,
        id: "texture",
        prompts: &[
            "an ambient texture",
            "an atmospheric drone",
            "background ambience",
            "a field recording",
            "an atmosphere",
        ],
        words: &[
            "texture",
            "atmosphere",
            "atmo",
            "atmos",
            "ambience",
            "drone",
            "soundscape",
            "foley",
        ],
        weak_words: &[],
    },
];

impl Category {
    /// Every category, in table order.
    pub const ALL: [Category; N_CATEGORIES] = [
        Category::Kick,
        Category::Snare,
        Category::Clap,
        Category::Hat,
        Category::Cymbal,
        Category::Tom,
        Category::Perc,
        Category::Drums,
        Category::Bass,
        Category::Lead,
        Category::Pad,
        Category::Keys,
        Category::Pluck,
        Category::Synth,
        Category::Vocal,
        Category::Fx,
        Category::Texture,
    ];

    /// Index in [`TAXONOMY`] / [`Category::ALL`].
    pub fn index(self) -> usize {
        self as usize
    }

    /// Taxonomy row.
    pub fn info(self) -> &'static CategoryInfo {
        &TAXONOMY[self.index()]
    }

    /// Stable id (`kick`, `hat`, ...).
    pub fn as_str(self) -> &'static str {
        self.info().id
    }

    /// Parses an id, case-insensitively, also accepting a few common
    /// spellings of the ids (`hihat`, `hi-hat`, `percussion`, `fx`, ...),
    /// the words of the taxonomy.
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim().to_ascii_lowercase().replace(['-', '_', ' '], "");
        if s.is_empty() {
            return None;
        }
        let singular = s.strip_suffix('s').unwrap_or(&s);
        TAXONOMY
            .iter()
            .find(|i| {
                i.id == s
                    || i.id == singular
                    || i.words.iter().any(|w| w.replace(' ', "") == s)
                    || i.words.iter().any(|w| w.replace(' ', "") == singular)
            })
            .map(|i| i.category)
    }
}

impl std::fmt::Display for Category {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
