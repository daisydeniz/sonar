//! Minimal RFC 4180 CSV reader/writer for `labels.csv` (quoted fields,
//! doubled quotes, CRLF, optional UTF-8 BOM).

/// Parses `text` into rows of fields. Blank lines are skipped. An
/// unterminated quote ends at end of input (lenient).
pub fn parse(text: &str) -> Vec<Vec<String>> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut rows = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    let finish_row = |row: &mut Vec<String>, field: &mut String, rows: &mut Vec<Vec<String>>| {
        row.push(std::mem::take(field));
        let blank = row.len() == 1 && row[0].is_empty();
        let done = std::mem::take(row);
        if !blank {
            rows.push(done);
        }
    };
    while let Some(c) = chars.next() {
        match (c, quoted) {
            ('"', true) if chars.peek() == Some(&'"') => {
                chars.next();
                field.push('"');
            }
            ('"', true) => quoted = false,
            ('"', false) if field.is_empty() => quoted = true,
            (',', false) => row.push(std::mem::take(&mut field)),
            ('\r', false) if chars.peek() == Some(&'\n') => {}
            ('\n' | '\r', false) => finish_row(&mut row, &mut field, &mut rows),
            (c, _) => field.push(c),
        }
    }
    if !field.is_empty() || !row.is_empty() {
        finish_row(&mut row, &mut field, &mut rows);
    }
    rows
}

/// One field, quoted when it contains `,`, `"` or a line break.
pub fn escape(field: &str) -> String {
    if field.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_quotes_crlf_and_bom() {
        let text = "\u{feff}path,notes\r\n\"a, b.wav\",\"say \"\"hi\"\"\"\r\n\r\nc.wav,\n";
        assert_eq!(
            parse(text),
            vec![
                vec!["path", "notes"],
                vec!["a, b.wav", "say \"hi\""],
                vec!["c.wav", ""],
            ]
        );
        assert_eq!(parse("x,y"), vec![vec!["x", "y"]]);
        assert!(parse("").is_empty());
    }

    #[test]
    fn escape_round_trips() {
        for s in ["plain", "a,b", "q\"uote", "multi\nline", ""] {
            let line = format!("{},end", escape(s));
            assert_eq!(parse(&line), vec![vec![s.to_string(), "end".to_string()]]);
        }
    }
}
