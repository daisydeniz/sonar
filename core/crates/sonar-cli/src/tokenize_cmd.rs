//! `sonar tokenize`: show how names/paths/queries are tokenized for search,
//! plus the BPM/key priors read from the file name.

use std::path::Path;

use anyhow::Result;
use serde::Serialize;
use sonar_core::text::{
    filename_prior, strip_audio_extension, FilenamePrior, PriorSource, Token, Tokenizer,
};

#[derive(Serialize)]
struct Report<'a> {
    input: &'a str,
    tokens: Vec<Token>,
    prior: FilenamePrior,
}

/// Tokenizes every input (a media extension is dropped first; query mode
/// if `query`) and prints tokens and priors as text or a JSON array.
pub fn run(inputs: &[String], query: bool, json: bool) -> Result<()> {
    let mut tokenizer = if query {
        Tokenizer::for_query()
    } else {
        Tokenizer::default()
    };
    let reports: Vec<_> = inputs
        .iter()
        .map(|input| Report {
            input,
            tokens: tokenizer.tokenize(strip_audio_extension(input)),
            prior: filename_prior(Path::new(input)),
        })
        .collect();
    if json {
        println!("{}", serde_json::to_string_pretty(&reports)?);
    } else {
        for r in &reports {
            println!("{}", r.input);
            println!("  tokens: {}", render_tokens(&r.tokens));
            println!("  prior:  {}", render_prior(&r.prior));
        }
    }
    Ok(())
}

/// Positions separated by spaces; tokens sharing a position joined by `|`.
fn render_tokens(tokens: &[Token]) -> String {
    let mut s = String::new();
    let mut pos = None;
    for t in tokens {
        if pos == Some(t.position) {
            s.push('|');
        } else if pos.is_some() {
            s.push(' ');
        }
        pos = Some(t.position);
        s.push_str(&t.text);
    }
    s
}

fn render_prior(p: &FilenamePrior) -> String {
    let how = |explicit: bool, source: PriorSource| {
        let source = match source {
            PriorSource::Filename => "filename",
            PriorSource::Directory => "folder",
        };
        if explicit {
            source.to_owned()
        } else {
            format!("bare, {source}")
        }
    };
    let bpm = p.bpm.map_or_else(
        || "bpm -".to_owned(),
        |b| format!("bpm {} ({})", b.bpm, how(b.explicit, b.source)),
    );
    let key = p.key.map_or_else(
        || "key -".to_owned(),
        |k| format!("key {} ({})", k.key, how(k.explicit, k.source)),
    );
    format!("{bpm}, {key}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_tokens_and_prior() {
        let mut t = Tokenizer::default();
        let name = "Bass_Loop_128_Am.wav";
        let toks = t.tokenize(strip_audio_extension(name));
        assert_eq!(
            render_tokens(&toks),
            "bass loop 128bpm|128 key:am|note:a|am"
        );
        assert_eq!(
            render_prior(&filename_prior(Path::new(name))),
            "bpm 128 (bare, filename), key Am (filename)"
        );
    }
}
