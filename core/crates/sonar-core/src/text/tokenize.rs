//! Tokenizer for file names, paths and queries.
//!
//! Pipeline: [`super::lex`] (pieces) → [`super::recognize`] (units with
//! music/noise rules) → tokens. Each emitting unit takes one *position*;
//! extra tokens for the same unit share it (position increment 0, like
//! Lucene synonyms), so T2.2 can wrap this in a Tantivy `TokenStream`.
//!
//! Tokens per unit:
//! - word → folded text (dropped if a stop word);
//! - number → digits (`808`);
//! - BPM → `120bpm` (kind `Bpm`) + `120` (kind `Number`, integers only);
//! - key → `key:c#m` (kind `Key`) + `note:c#` (kind `Note`) + the folded
//!   original if the key was written as one word (`c#m`, `gbmaj`,
//!   `aminor`), so a lowercase query word like `am` still matches `Am`;
//! - camelCase runs also emit the whole run folded at the position of their
//!   first token (`DnB` → `dn` + `dnb`, `b`), like ES `catenate_all` +
//!   `preserve_original`.

use serde::Serialize;

use super::lex::{fold, fold_into, lex, Piece};
use super::recognize::{recognize, NoiseRules, Unit, UnitKind};
use super::stopwords::StopWords;

/// Kind of a [`Token`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TokenKind {
    /// Folded word (or catenated camelCase run, or original key spelling).
    Word,
    /// Plain integer.
    Number,
    /// Canonical tempo, `<n>bpm` (`120bpm`, `92.5bpm`).
    Bpm,
    /// Canonical key, `key:<root>[m]` with sharp roots (`key:a#m`).
    Key,
    /// Canonical note of a key, `note:<root>` (`note:a#`).
    Note,
}

/// One token with its byte span in the input and its position.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Token {
    /// Normalised token text.
    pub text: String,
    /// What the token is.
    pub kind: TokenKind,
    /// Byte offset of the source span start in the input.
    pub start: usize,
    /// Byte offset one past the source span end.
    pub end: usize,
    /// Position; tokens from the same source span share it.
    pub position: u32,
}

/// Tokenizer settings.
#[derive(Debug, Clone)]
pub struct TokenizerConfig {
    /// Pack-noise words to drop (see [`super::stopwords`]).
    pub stop_words: StopWords,
    /// Drop zero-padded numbers (`01`, `002`: take/index numbers).
    pub drop_padded_numbers: bool,
    /// Drop version markers (`v2`, `V1.2`).
    pub drop_versions: bool,
    /// Query mode: all-lowercase key spellings (`am`, `cm`, `eb`, `dbm`)
    /// also emit `key:`/`note:` tokens next to the plain word, because
    /// users type queries in lowercase. Off for indexing, where names keep
    /// the strict case rules (`fm bass` is FM, not F minor). Callers must OR
    /// tokens that share a position when building a query.
    pub query: bool,
}

impl Default for TokenizerConfig {
    /// Index mode with the default stop list.
    fn default() -> Self {
        Self {
            stop_words: StopWords::default(),
            drop_padded_numbers: true,
            drop_versions: true,
            query: false,
        }
    }
}

impl TokenizerConfig {
    /// Default settings in query mode.
    pub fn query() -> Self {
        Self {
            query: true,
            ..Self::default()
        }
    }

    pub(crate) fn noise_rules(&self) -> NoiseRules {
        NoiseRules {
            drop_padded_numbers: self.drop_padded_numbers,
            drop_versions: self.drop_versions,
            query: self.query,
        }
    }
}

/// Reusable tokenizer (keeps scratch buffers between calls).
#[derive(Debug, Clone, Default)]
pub struct Tokenizer {
    config: TokenizerConfig,
    pieces: Vec<Piece>,
    units: Vec<Unit>,
}

impl Tokenizer {
    /// A tokenizer with `config`.
    pub fn new(config: TokenizerConfig) -> Self {
        Self {
            config,
            pieces: Vec::new(),
            units: Vec::new(),
        }
    }

    /// A tokenizer for search queries ([`TokenizerConfig::query`]).
    pub fn for_query() -> Self {
        Self::new(TokenizerConfig::query())
    }

    /// The settings in use.
    pub fn config(&self) -> &TokenizerConfig {
        &self.config
    }

    /// Tokens of `text`.
    pub fn tokenize(&mut self, text: &str) -> Vec<Token> {
        let mut out = Vec::new();
        self.tokenize_into(text, &mut out);
        out
    }

    /// Appends the tokens of `text` to `out` (positions start at 0).
    pub fn tokenize_into(&mut self, text: &str, out: &mut Vec<Token>) {
        lex(text, &mut self.pieces);
        recognize(
            text,
            &self.pieces,
            self.config.noise_rules(),
            &mut self.units,
        );
        let mut position = 0u32;
        let mut catenated_run = None;
        for u in &self.units {
            let before = out.len();
            let mut push = |text: String, kind| {
                out.push(Token {
                    text,
                    kind,
                    start: u.start,
                    end: u.end,
                    position,
                })
            };
            let raw = &text[u.start..u.end];
            match u.kind {
                UnitKind::Noise | UnitKind::BareNote(_) => {}
                UnitKind::Word => {
                    let w = fold(raw);
                    // Empty after folding: a lone combining mark.
                    if !w.is_empty() && !self.config.stop_words.contains(&w) {
                        push(w, TokenKind::Word);
                    }
                }
                UnitKind::Number => push(raw.to_owned(), TokenKind::Number),
                UnitKind::Bpm { value, .. } => {
                    push(format!("{value}bpm"), TokenKind::Bpm);
                    if value.fract() == 0.0 {
                        push(format!("{value}"), TokenKind::Number);
                    }
                }
                UnitKind::Key { key, .. } => {
                    push(key.token(), TokenKind::Key);
                    push(key.note_token(), TokenKind::Note);
                    if self.pieces[u.first].run == self.pieces[u.last].run {
                        push(fold(raw), TokenKind::Word);
                    }
                }
            }
            if out.len() == before {
                continue;
            }
            let run = self.pieces[u.first].run;
            if catenated_run != Some(run) {
                catenated_run = Some(run);
                if let Some((cat, start, end)) = catenation(text, &self.pieces, u) {
                    out.push(Token {
                        text: cat,
                        kind: TokenKind::Word,
                        start,
                        end,
                        position,
                    });
                }
            }
            position += 1;
        }
    }
}

/// Folded text and byte span of `u`'s run if the run was split by case and
/// `u` does not cover it entirely.
fn catenation(text: &str, pieces: &[Piece], u: &Unit) -> Option<(String, usize, usize)> {
    let run = pieces[u.first].run;
    let first = pieces[..u.first]
        .iter()
        .rposition(|p| p.run != run)
        .map_or(0, |k| k + 1);
    let last = u.first
        + pieces[u.first..]
            .iter()
            .take_while(|p| p.run == run)
            .count()
        - 1;
    let camel = pieces[first..=last].iter().any(|p| p.case_split);
    if !camel || (first >= u.first && last <= u.last) {
        return None;
    }
    let (start, end) = (pieces[first].start, pieces[last].end);
    let mut s = String::new();
    fold_into(&text[start..end], &mut s);
    (!s.is_empty()).then_some((s, start, end))
}

/// Tokens of `text` with the default configuration.
pub fn tokenize(text: &str) -> Vec<Token> {
    Tokenizer::default().tokenize(text)
}

/// Media file extensions removed by [`strip_audio_extension`].
const MEDIA_EXTENSIONS: &[&str] = &[
    "aac", "aif", "aifc", "aiff", "alac", "caf", "flac", "m4a", "mid", "midi", "mp3", "mp4", "oga",
    "ogg", "opus", "rex", "rx2", "wav", "wave", "wv",
];

/// `name` without a trailing media extension (`Kick.Hard.wav` →
/// `Kick.Hard`; `Loop 92.5 BPM` is unchanged).
pub fn strip_audio_extension(name: &str) -> &str {
    match name.rsplit_once('.') {
        Some((stem, ext))
            if !stem.is_empty() && MEDIA_EXTENSIONS.iter().any(|e| e.eq_ignore_ascii_case(ext)) =>
        {
            stem
        }
        _ => name,
    }
}
