//! Text analysis for search: tokenizer for file names, paths and queries,
//! plus BPM/key priors read from file names.
//!
//! Analysis chain (same for indexing and queries, so both sides agree):
//! 1. split on separators (`_ - . space / \ ( ) [ ]` …), letter↔digit and
//!    camelCase boundaries (`TR808Kick` → `tr 808 kick`);
//! 2. lowercase and fold diacritics (`Café` → `cafe`, NFD-safe);
//! 3. recognise music tokens: `120bpm` / `120 BPM` / `BPM_120` →
//!    `120bpm`; keys `Cmin`, `C#m`, `F# minor`, `Gbmaj`, `Bb` →
//!    `key:<root>[m]` + `note:<root>` with sharp roots (`key:cm`,
//!    `key:c#m`, `key:f#m`, `key:f#`, `key:a#`);
//! 4. drop pack noise: stop words ([`DEFAULT_STOP_WORDS`]), zero-padded take
//!    numbers (`_01`), versions (`v2`), `24bit`/`44.1kHz`/`48k`, `44100`.
//!
//! Synonyms (`docs/synonyms.tsv`) and the Tantivy wrapper come in T2.2.
//! Recognition rules: see `recognize.rs` and [`music`].

mod lex;
pub mod music;
mod priors;
mod recognize;
mod stopwords;
mod tokenize;

#[cfg(test)]
mod priors_tests;
#[cfg(test)]
mod tokenize_tests;

pub use music::{Key, Mode};
pub use priors::{
    filename_prior, BpmPrior, FilenamePrior, KeyPrior, PriorSource, PRIOR_BPM_RANGE,
    PRIOR_DIR_LEVELS,
};
pub use stopwords::{StopWords, DEFAULT_STOP_WORDS};
pub use tokenize::{strip_audio_extension, tokenize, Token, TokenKind, Tokenizer, TokenizerConfig};
