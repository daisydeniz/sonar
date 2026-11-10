//! Tantivy tokenizer over [`crate::text::Tokenizer`] (index mode).
//!
//! Positions and byte offsets are passed through unchanged, including the
//! shared positions of alternative tokens (`key:am|note:a|am`,
//! `kicks|kick`), so phrase queries see the same adjacency as the text
//! chain. A token repeated at one position is emitted once.

use tantivy::tokenizer::{Token as TantivyToken, TokenStream, Tokenizer as TantivyTokenizer};

use crate::text::{Token, Tokenizer};

/// Name the analyzer is registered under in every Sonar index.
pub(crate) const TOKENIZER_NAME: &str = "sonar";

/// The Sonar analysis chain as a Tantivy tokenizer.
#[derive(Debug, Clone, Default)]
pub struct SonarTokenizer {
    inner: Tokenizer,
    tokens: Vec<Token>,
}

/// Token stream of [`SonarTokenizer`].
#[derive(Debug)]
pub struct SonarTokenStream<'a> {
    tokens: &'a [Token],
    next: usize,
    token: TantivyToken,
}

impl TantivyTokenizer for SonarTokenizer {
    type TokenStream<'a> = SonarTokenStream<'a>;

    fn token_stream<'a>(&'a mut self, text: &'a str) -> SonarTokenStream<'a> {
        self.tokens.clear();
        self.inner.tokenize_into(text, &mut self.tokens);
        dedup_per_position(&mut self.tokens);
        SonarTokenStream {
            tokens: &self.tokens,
            next: 0,
            token: TantivyToken::default(),
        }
    }
}

/// Drops a token whose text already occurred at the same position (e.g. a
/// singular equal to a catenation): it would only inflate the term
/// frequency. Tokens of one position are contiguous.
fn dedup_per_position(tokens: &mut Vec<Token>) {
    let mut kept: Vec<Token> = Vec::with_capacity(tokens.len());
    let mut group_start = 0;
    for t in tokens.drain(..) {
        if kept.last().is_some_and(|last| last.position != t.position) {
            group_start = kept.len();
        }
        if !kept[group_start..].iter().any(|k| k.text == t.text) {
            kept.push(t);
        }
    }
    *tokens = kept;
}

impl TokenStream for SonarTokenStream<'_> {
    fn advance(&mut self) -> bool {
        let Some(t) = self.tokens.get(self.next) else {
            return false;
        };
        self.next += 1;
        self.token.text.clear();
        self.token.text.push_str(&t.text);
        self.token.offset_from = t.start;
        self.token.offset_to = t.end;
        self.token.position = t.position as usize;
        self.token.position_length = 1;
        true
    }

    fn token(&self) -> &TantivyToken {
        &self.token
    }

    fn token_mut(&mut self) -> &mut TantivyToken {
        &mut self.token
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `(text, position, from, to)` of every token of `text`.
    fn stream(text: &str) -> Vec<(String, usize, usize, usize)> {
        let mut t = SonarTokenizer::default();
        let mut s = t.token_stream(text);
        let mut out = Vec::new();
        while let Some(tok) = s.next() {
            out.push((
                tok.text.clone(),
                tok.position,
                tok.offset_from,
                tok.offset_to,
            ));
        }
        out
    }

    #[test]
    fn passes_positions_and_offsets_through() {
        let got = stream("Bass_Am HiHats");
        let want = [
            ("bass", 0, 0, 4),
            ("key:am", 1, 5, 7),
            ("note:a", 1, 5, 7),
            ("am", 1, 5, 7),
            ("hi", 2, 8, 10),
            ("hihats", 2, 8, 14),
            ("hihat", 2, 8, 14),
            ("hats", 3, 10, 14),
            ("hat", 3, 10, 14),
        ];
        let want: Vec<_> = want
            .iter()
            .map(|&(t, p, a, b)| (t.to_owned(), p, a, b))
            .collect();
        assert_eq!(got, want);
        assert!(stream("").is_empty());
        assert!(stream("the.wav").is_empty());
    }

    #[test]
    fn dedups_only_within_a_position() {
        let tok = |text: &str, position| Token {
            text: text.into(),
            kind: crate::text::TokenKind::Word,
            start: 0,
            end: 1,
            position,
        };
        let mut v = vec![
            tok("a", 0),
            tok("b", 0),
            tok("a", 0),
            tok("a", 1),
            tok("a", 1),
            tok("c", 2),
        ];
        dedup_per_position(&mut v);
        let got: Vec<_> = v.iter().map(|t| (t.text.as_str(), t.position)).collect();
        assert_eq!(got, [("a", 0), ("b", 0), ("a", 1), ("c", 2)]);
    }
}
