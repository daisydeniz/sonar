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
//!    numbers (`_01`), versions (`v2`), `24bit`/`44.1kHz`/`48k`, `44100`;
//! 5. add the singular of plural words at the same position (`kicks|kick`,
//!    `plural.rs`).
//!
//! Synonyms (`docs/synonyms.tsv`) are expanded at query time by
//! [`crate::search`]; the Tantivy wrapper is in [`crate::index`].
//! Recognition rules: see `recognize.rs` and [`music`].

mod lex;
pub mod music;
mod plural;
mod priors;
mod recognize;
mod stopwords;
mod tokenize;

#[cfg(test)]
mod priors_tests;
#[cfg(test)]
mod tokenize_tests;

/// Version of the analysis chain. Bump whenever the tokens produced for a
/// given input change: the text index is rebuilt on a mismatch (ADR-017).
/// 1 = T2.1, 2 = plural folding (T2.2).
pub const ANALYZER_VERSION: u32 = 2;

pub(crate) use lex::fold;
pub use music::{Key, Mode};
pub use priors::{
    filename_prior, BpmPrior, FilenamePrior, KeyPrior, PriorSource, PRIOR_BPM_RANGE,
    PRIOR_DIR_LEVELS,
};
pub use stopwords::{StopWords, DEFAULT_STOP_WORDS};
pub use tokenize::{strip_audio_extension, tokenize, Token, TokenKind, Tokenizer, TokenizerConfig};
