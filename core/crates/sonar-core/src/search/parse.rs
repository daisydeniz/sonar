//! Query syntax: whitespace-separated chunks; `-chunk` excludes,
//! `"several words"` groups words (`-"one shot"` excludes the group).
//! Everything else is plain text for the analysis chain.

/// One chunk of a query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Chunk<'a> {
    /// Text without the leading `-` and the quotes.
    pub text: &'a str,
    /// Written as `-text` / `-"text"`.
    pub negated: bool,
    /// Written in quotes.
    pub quoted: bool,
}

/// A query split into chunks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Parsed<'a> {
    /// Non-empty chunks in query order.
    pub chunks: Vec<Chunk<'a>>,
    /// The last chunk runs to the end of the query (no trailing space or
    /// closing quote), i.e. the user may still be typing it.
    pub open_end: bool,
}

/// Splits `q` into chunks. Never fails: a lone `-` or `""` is ignored, an
/// unterminated quote runs to the end.
pub(crate) fn parse(q: &str) -> Parsed<'_> {
    let mut chunks = Vec::new();
    let mut open_end = false;
    let mut i = 0;
    let len = q.len();
    while i < len {
        let rest = &q[i..];
        let skipped = rest.len() - rest.trim_start().len();
        i += skipped;
        if i >= len {
            open_end = false;
            break;
        }
        let negated = q[i..].starts_with('-');
        if negated {
            i += 1;
        }
        let quoted = q[i..].starts_with('"');
        let (text, next, ends_open) = if quoted {
            let start = i + 1;
            match q[start..].find('"') {
                Some(n) => (&q[start..start + n], start + n + 1, false),
                None => (&q[start..], len, true),
            }
        } else {
            let n = q[i..].find(char::is_whitespace).unwrap_or(len - i);
            (&q[i..i + n], i + n, i + n == len)
        };
        open_end = ends_open;
        i = next;
        if !text.trim().is_empty() {
            chunks.push(Chunk {
                text,
                negated,
                quoted,
            });
        } else if ends_open {
            open_end = false;
        }
    }
    Parsed { chunks, open_end }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn show(q: &str) -> (Vec<String>, bool) {
        let p = parse(q);
        let chunks = p
            .chunks
            .iter()
            .map(|c| {
                let neg = if c.negated { "-" } else { "" };
                if c.quoted {
                    format!("{neg}\"{}\"", c.text)
                } else {
                    format!("{neg}{}", c.text)
                }
            })
            .collect();
        (chunks, p.open_end)
    }

    #[test]
    fn parse_table() {
        let cases: &[(&str, &[&str], bool)] = &[
            ("", &[], false),
            ("   ", &[], false),
            ("kick", &["kick"], true),
            ("kick ", &["kick"], false),
            ("  dark   kick", &["dark", "kick"], true),
            ("kick -vinyl", &["kick", "-vinyl"], true),
            ("lo-fi kick", &["lo-fi", "kick"], true),
            ("kick - ", &["kick"], false),
            ("kick -", &["kick"], false),
            ("-\"one shot\" kick", &["-\"one shot\"", "kick"], true),
            ("\"one shot\"", &["\"one shot\""], false),
            ("kick \"one sh", &["kick", "\"one sh\""], true),
            ("kick \"\"", &["kick"], false),
            ("Café\u{00a0}Bass", &["Café", "Bass"], true),
        ];
        for &(q, chunks, open) in cases {
            let (got, got_open) = show(q);
            assert_eq!(got, chunks, "{q:?}");
            assert_eq!(got_open, open, "{q:?}");
        }
    }
}
