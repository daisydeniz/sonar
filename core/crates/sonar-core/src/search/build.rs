//! Query text → Tantivy query (semantics in ADR-017).
//!
//! - Positive text is analyzed as a whole with the query-mode chain, so
//!   `120 bpm` or `C minor` across words still become `120bpm` / `key:cm`.
//! - Each query position is one clause: a dis-max over its alternatives
//!   (tokens sharing the position, single-word synonyms, multi-word
//!   synonyms as phrases) × the text fields with their weights. Positions
//!   are ANDed: every word narrows.
//! - A multi-word synonym member matching consecutive positions (`bass
//!   drum`, `one shot`) replaces them with one clause: those positions
//!   ANDed, or any other member of the group.
//! - `note:` tokens are not used: they would make `Am` match A major.
//! - As-you-type: when the query does not end in a space, the last chunk
//!   also matches as a prefix (constant score, below exact matches). If it
//!   was split (`120b` → `120`, `b`) or dropped by the analysis, the
//!   folded chunk itself is a prefix alternative (`120b*` → `120bpm`). A
//!   last chunk that analyzes to nothing or could still become a stop word
//!   (`bp` → `bpm`) only boosts and does not narrow.
//! - `-chunk` → must-not of the chunk's clauses (with synonyms).
//! - No positive clause (empty query, only exclusions/stop words) → all
//!   documents, minus exclusions.

use tantivy::query::{
    AllQuery, BooleanQuery, BoostQuery, DisjunctionMaxQuery, Occur, PhraseQuery, Query, RegexQuery,
    TermQuery,
};
use tantivy::schema::IndexRecordOption;
use tantivy::Term;

use super::parse::parse;
use super::synonyms::Synonyms;
use crate::index::Fields;
use crate::text::{fold, StopWords, Token, TokenKind, Tokenizer};
use crate::Result;

/// Weight of a synonym relative to what the user typed.
const SYNONYM_WEIGHT: f32 = 0.8;

/// Constant score of a prefix-only match, times the field weight. Below a
/// typical exact BM25 match, so `kick` ranks `Kick_01` above `Kickstart`.
const PREFIX_WEIGHT: f32 = 0.5;

/// Dis-max tie breaker: a little credit for matching in several fields
/// (name and folder) or several alternatives.
const TIE_BREAKER: f32 = 0.1;

/// A built query plus the term groups whose IDF is blended.
pub(crate) struct Built {
    /// The query.
    pub query: Box<dyn Query>,
    /// Per position and field, the single terms that are alternatives.
    pub blend: Vec<Vec<Term>>,
    /// The query has positive text, so its scores mean relevance (else
    /// every match scores the same).
    pub scored: bool,
}

/// One query position: its alternative token texts and source offset.
struct Slot {
    alts: Vec<String>,
    start: usize,
}

/// Builds the query for `text`. `prefix` enables as-you-type matching of
/// an unfinished last chunk.
pub(crate) fn build(text: &str, prefix: bool, syn: &Synonyms, fields: &Fields) -> Result<Built> {
    let parsed = parse(text);
    let mut b = Builder {
        fields,
        syn,
        blend: Vec::new(),
    };
    let mut tokenizer = Tokenizer::for_query();
    let mut clauses: Vec<(Occur, Box<dyn Query>)> = Vec::new();

    // Positive chunks, joined; remember where an unfinished tail starts.
    let mut positive = String::new();
    let mut tail_start = None;
    let last = parsed.chunks.len().saturating_sub(1);
    for (i, c) in parsed.chunks.iter().enumerate().filter(|(_, c)| !c.negated) {
        if !positive.is_empty() {
            positive.push(' ');
        }
        if prefix && parsed.open_end && i == last {
            tail_start = Some(positive.len());
        }
        positive.push_str(c.text);
    }
    let slots = to_slots(&tokenizer.tokenize(&positive));
    let split = tail_start.map_or(slots.len(), |ts| {
        slots
            .iter()
            .position(|s| s.start >= ts)
            .unwrap_or(slots.len())
    });
    for q in b.and_parts(&slots[..split], false)? {
        clauses.push((Occur::Must, q));
    }
    if let Some(ts) = tail_start {
        let tail = &slots[split..];
        let raw = prefix_key(&positive[ts..]);
        let stop = tokenizer.config().stop_words.clone();
        if let Some((occur, q)) = b.tail_clause(tail, &raw, &stop)? {
            clauses.push((occur, q));
        }
    }
    let scored = !clauses.is_empty();
    if !clauses.iter().any(|(o, _)| *o == Occur::Must) {
        clauses.push((Occur::Must, Box::new(AllQuery)));
    }

    for q in exclusion_parts(&parsed, &mut b, &mut tokenizer)? {
        clauses.push((Occur::MustNot, q));
    }
    Ok(Built {
        query: Box::new(BooleanQuery::new(clauses)),
        blend: b.blend,
        scored,
    })
}

/// The `-chunk` exclusions of `text` as [`build`] applies them (one query
/// per chunk, synonyms included), for callers that restrict a candidate
/// set without the positive text (the semantic leg of hybrid search).
pub(crate) fn exclusions(
    text: &str,
    syn: &Synonyms,
    fields: &Fields,
) -> Result<Vec<Box<dyn Query>>> {
    let mut b = Builder {
        fields,
        syn,
        blend: Vec::new(),
    };
    exclusion_parts(&parse(text), &mut b, &mut Tokenizer::for_query())
}

/// Exclusions, each chunk on its own.
fn exclusion_parts(
    parsed: &super::parse::Parsed<'_>,
    b: &mut Builder<'_>,
    tokenizer: &mut Tokenizer,
) -> Result<Vec<Box<dyn Query>>> {
    let mut out = Vec::new();
    for c in parsed.chunks.iter().filter(|c| c.negated) {
        let slots = to_slots(&tokenizer.tokenize(c.text));
        let parts = b.and_parts(&slots, false)?;
        if !parts.is_empty() {
            out.push(and(parts));
        }
    }
    Ok(out)
}

/// Groups tokens by position, dropping `note:` tokens and duplicates.
fn to_slots(tokens: &[Token]) -> Vec<Slot> {
    let mut slots: Vec<Slot> = Vec::new();
    let mut pos = None;
    for t in tokens.iter().filter(|t| t.kind != TokenKind::Note) {
        if pos != Some(t.position) {
            pos = Some(t.position);
            slots.push(Slot {
                alts: Vec::new(),
                start: t.start,
            });
        }
        if let Some(s) = slots.last_mut() {
            if !s.alts.contains(&t.text) {
                s.alts.push(t.text.clone());
            }
            s.start = s.start.min(t.start);
        }
    }
    slots
}

/// Folded chunk with only letters, digits and `#` (`120B` → `120b`,
/// `lo-fi` → `lofi`), for whole-chunk prefix matching.
fn prefix_key(raw: &str) -> String {
    fold(raw)
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '#')
        .collect()
}

/// Conjunction of `parts` (a single part as is).
fn and(mut parts: Vec<Box<dyn Query>>) -> Box<dyn Query> {
    if parts.len() == 1 {
        if let Some(p) = parts.pop() {
            return p;
        }
    }
    Box::new(BooleanQuery::intersection(parts))
}

fn dismax(disjuncts: Vec<Box<dyn Query>>) -> Box<dyn Query> {
    Box::new(DisjunctionMaxQuery::with_tie_breaker(
        disjuncts,
        TIE_BREAKER,
    ))
}

struct Builder<'a> {
    fields: &'a Fields,
    syn: &'a Synonyms,
    blend: Vec<Vec<Term>>,
}

impl Builder<'_> {
    /// One required clause per position or multi-word synonym span; the
    /// last slot also matches as a prefix if `prefix_last`.
    fn and_parts(&mut self, slots: &[Slot], prefix_last: bool) -> Result<Vec<Box<dyn Query>>> {
        let alts: Vec<Vec<String>> = slots.iter().map(|s| s.alts.clone()).collect();
        let n = slots.len();
        let mut parts = Vec::new();
        let mut i = 0;
        while i < n {
            let is_prefix = |j: usize| prefix_last && j + 1 == n;
            if let Some((k, groups)) = self.syn.longest_match(&alts, i, 2) {
                let inner = (i..i + k)
                    .map(|j| self.slot_clause(&slots[j], is_prefix(j)))
                    .collect::<Result<_>>()?;
                let mut disj = vec![and(inner)];
                for g in groups {
                    for m in self.syn.members(g) {
                        self.member_disjuncts(m, &mut disj);
                    }
                }
                parts.push(dismax(disj));
                i += k;
            } else {
                parts.push(self.slot_clause(&slots[i], is_prefix(i))?);
                i += 1;
            }
        }
        Ok(parts)
    }

    /// Dis-max over the alternatives of one position, its single-word
    /// synonym groups and (if `prefix`) prefix matches of its alternatives.
    fn slot_clause(&mut self, slot: &Slot, prefix: bool) -> Result<Box<dyn Query>> {
        let mut singles: Vec<(String, f32)> = slot.alts.iter().map(|a| (a.clone(), 1.0)).collect();
        let mut disj = Vec::new();
        if let Some((_, groups)) = self
            .syn
            .longest_match(std::slice::from_ref(&slot.alts), 0, 1)
        {
            for g in groups {
                for m in self.syn.members(g) {
                    match m.as_slice() {
                        [one] if !singles.iter().any(|(s, _)| s == one) => {
                            singles.push((one.clone(), SYNONYM_WEIGHT));
                        }
                        [_] => {}
                        _ => self.phrase_disjuncts(m, SYNONYM_WEIGHT, &mut disj),
                    }
                }
            }
        }
        for tf in self.fields.text_fields() {
            let terms: Vec<Term> = singles
                .iter()
                .map(|(s, _)| Term::from_field_text(tf.field, s))
                .collect();
            for (term, (_, w)) in terms.iter().zip(&singles) {
                let q = TermQuery::new(term.clone(), IndexRecordOption::WithFreqs);
                disj.push(Box::new(BoostQuery::new(Box::new(q), w * tf.weight)));
            }
            self.blend.push(terms);
        }
        if prefix {
            let prefixes: Vec<&str> = slot.alts.iter().map(String::as_str).collect();
            disj.extend(self.prefix_disjuncts(&prefixes)?);
        }
        Ok(dismax(disj))
    }

    /// The unfinished last chunk: its positions with the last as prefix,
    /// or the whole folded chunk as prefix. Optional (boost only) when the
    /// chunk analyzed to nothing or may become a stop word.
    fn tail_clause(
        &mut self,
        tail: &[Slot],
        raw: &str,
        stop: &StopWords,
    ) -> Result<Option<(Occur, Box<dyn Query>)>> {
        let whole_chunk_prefix = !raw.is_empty()
            && (tail.len() != 1 || !tail.iter().any(|s| s.alts.iter().any(|a| a == raw)));
        let mut disj = Vec::new();
        if !tail.is_empty() {
            disj.push(and(self.and_parts(tail, true)?));
        }
        if whole_chunk_prefix {
            disj.extend(self.prefix_disjuncts(&[raw])?);
        }
        if disj.is_empty() {
            return Ok(None);
        }
        let optional = tail.is_empty() || (!raw.is_empty() && stop.any_starts_with(raw));
        let occur = if optional { Occur::Should } else { Occur::Must };
        Ok(Some((occur, dismax(disj))))
    }

    /// A synonym member as term (one word) or phrase alternative.
    fn member_disjuncts(&mut self, member: &[String], disj: &mut Vec<Box<dyn Query>>) {
        match member {
            [one] => {
                for tf in self.fields.text_fields() {
                    let q = TermQuery::new(
                        Term::from_field_text(tf.field, one),
                        IndexRecordOption::WithFreqs,
                    );
                    disj.push(Box::new(BoostQuery::new(
                        Box::new(q),
                        SYNONYM_WEIGHT * tf.weight,
                    )));
                }
            }
            _ => self.phrase_disjuncts(member, SYNONYM_WEIGHT, disj),
        }
    }

    /// `words` (2+) as a phrase in every field.
    fn phrase_disjuncts(&self, words: &[String], weight: f32, disj: &mut Vec<Box<dyn Query>>) {
        if words.len() < 2 {
            return;
        }
        for tf in self.fields.text_fields() {
            let terms = words
                .iter()
                .map(|w| Term::from_field_text(tf.field, w))
                .collect();
            let q = PhraseQuery::new(terms);
            disj.push(Box::new(BoostQuery::new(Box::new(q), weight * tf.weight)));
        }
    }

    /// Constant-score prefix match of any of `prefixes` in every field:
    /// one term-dictionary automaton walk per field, no per-term scorers
    /// (so no expansion cap is needed; cost is the matching postings).
    fn prefix_disjuncts(&self, prefixes: &[&str]) -> Result<Vec<Box<dyn Query>>> {
        let alts: Vec<String> = prefixes
            .iter()
            .filter(|p| !p.is_empty())
            .map(|p| escape(p))
            .collect();
        if alts.is_empty() {
            return Ok(Vec::new());
        }
        let pattern = format!("(?:{}).*", alts.join("|"));
        let mut out: Vec<Box<dyn Query>> = Vec::new();
        for tf in self.fields.text_fields() {
            let q = RegexQuery::from_pattern(&pattern, tf.field)?;
            out.push(Box::new(BoostQuery::new(
                Box::new(q),
                PREFIX_WEIGHT * tf.weight,
            )));
        }
        Ok(out)
    }
}

/// `s` with regex meta characters escaped.
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for c in s.chars() {
        if "\\.+*?()|[]{}^$#&-~".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_group_positions_and_drop_notes() {
        let mut t = Tokenizer::for_query();
        let slots = to_slots(&t.tokenize("Am kicks"));
        let alts: Vec<_> = slots.iter().map(|s| s.alts.join("|")).collect();
        assert_eq!(alts, ["key:am|am", "kicks|kick"]);
        assert_eq!(slots[1].start, 3);
    }

    #[test]
    fn prefix_key_and_escape() {
        assert_eq!(prefix_key("120B"), "120b");
        assert_eq!(prefix_key("Lo-Fi"), "lofi");
        assert_eq!(prefix_key("C#"), "c#");
        assert_eq!(escape("key:c#"), "key:c\\#");
        assert_eq!(escape("92.5bpm"), "92\\.5bpm");
    }
}
