//! Table-driven tokenizer tests.

use super::*;

/// Tokens as text: positions separated by spaces, tokens sharing a
/// position joined by `|` (primary token first).
fn render(tokens: &[Token]) -> String {
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

#[test]
fn tokenize_table() {
    let cases: &[(&str, &str)] = &[
        // Separators, case, extensions-as-stop-words, take numbers.
        ("Deep_House-Kick 01.wav", "deep house kick"),
        ("Kick (Wet) [Dry]", "kick wet dry"),
        ("Drums/Kicks/Kick_01.wav", "drums|drum kicks|kick kick"),
        ("C:\\Samples\\Snare", "c snare"),
        ("One Shot Kick", "one shot kick"),
        ("dark kick -vinyl", "dark kick vinyl"),
        ("Don't Stop", "dont stop"),
        ("Kick#2", "kick 2"),
        ("", ""),
        ("___ --- ...", ""),
        // camelCase / acronyms / digits (camel runs also emit the whole run).
        ("DeepHouseKick", "deep|deephousekick house kick"),
        ("OneShot", "one|oneshot shot"),
        ("HiHat Closed", "hi|hihat hat closed"),
        ("DnB Break 174", "dn|dnb b break 174bpm|174"),
        ("TR808Kick", "tr 808 kick"),
        ("TR-909 Hat", "tr 909 hat"),
        ("Sub808", "sub 808"),
        // Diacritics.
        ("Café Crème", "cafe creme"),
        ("Cafe\u{301}", "cafe"),
        ("ÜBER BASS", "uber bass"),
        // Explicit BPM.
        ("Loop 120bpm", "loop 120bpm|120"),
        ("Loop 120 BPM", "loop 120bpm|120"),
        ("Loop_120_bpm", "loop 120bpm|120"),
        ("BPM120_Loop", "120bpm|120 loop"),
        ("120BPM_Am_Loop", "120bpm|120 key:am|note:a|am loop"),
        ("Pad 92.5bpm", "pad 92.5bpm"),
        ("Loop_120bpm_02", "loop 120bpm|120"),
        ("Loop bpm", "loop"),
        // Bare numbers: BPM only with tempo context.
        ("Kick 75", "kick 75"),
        ("Kick_120", "kick 120"),
        ("Bass Loop 128", "bass loop 128bpm|128"),
        ("Am 120", "key:am|note:a|am 120bpm|120"),
        (
            "SM101_Deep_Loop_124_Am",
            "sm 101 deep loop 124bpm|124 key:am|note:a|am",
        ),
        ("Loop 300", "loop 300"),
        ("Loop 050", "loop"),
        (
            "Bass Loop 128 F# 120",
            "bass loop 128 key:f#|note:f#|f# 120",
        ),
        ("Loops/128/Kick 2", "loops|loop 128 kick 2"),
        // Keys.
        ("Cmin Chord", "key:cm|note:c|cmin chord"),
        ("C#m", "key:c#m|note:c#|c#m"),
        ("C♯m Pad", "key:c#m|note:c#|c#m pad"),
        ("F# minor Pad", "key:f#m|note:f# pad"),
        ("Fsharp minor", "key:f#m|note:f#"),
        ("Gbmaj", "key:f#|note:f#|gbmaj"),
        ("GbMaj Keys", "key:f#|note:f#|gbmaj keys|key"),
        ("A minor", "key:am|note:a"),
        ("a min", "key:am|note:a"),
        ("AMinor", "key:am|note:a|aminor"),
        ("Bb", "key:a#|note:a#|bb"),
        ("Ebm Bass", "key:d#m|note:d#|ebm bass"),
        ("Dm7 Chord", "key:dm|note:d|dm 7 chord"),
        ("Cmaj7", "key:c|note:c|cmaj 7"),
        ("Bass Fm", "bass key:fm|note:f|fm"),
        // Not keys.
        ("A Kick", "a kick"),
        ("Kick A", "kick a"),
        ("FM Bass", "fm bass"),
        ("fm bass", "fm bass"),
        ("EBM Kick", "ebm kick"),
        ("-6dB", "6|6db d b"), // camel `dB` splits; no key
        ("Loop A 120", "loop a 120bpm|120"),
        // Bare note letter right after a BPM.
        ("Loop_120_C", "loop 120bpm|120 key:c|note:c|c"),
        // Technical noise and versions.
        ("24bit 44.1kHz Kick", "kick"),
        ("Kick 48k", "kick"),
        ("Kick_44100", "kick"),
        ("Snare v2", "snare"),
        ("Snare V1.2 Final", "snare final"),
        ("www.freesamples.com Kick", "freesamples|freesample kick"),
        ("Prod by DJ X", "dj x"),
    ];
    for (input, want) in cases {
        assert_eq!(render(&tokenize(input)), *want, "input {input:?}");
    }
}

#[test]
fn bpm_and_key_queries_match_their_index_forms() {
    // Same canonical tokens whatever the spelling.
    let canon = |s: &str| -> Vec<String> {
        tokenize(s)
            .into_iter()
            .filter(|t| matches!(t.kind, TokenKind::Bpm | TokenKind::Key))
            .map(|t| t.text)
            .collect()
    };
    for (a, b) in [
        ("120bpm", "120 BPM"),
        ("120_bpm", "BPM 120"),
        ("Cmin", "C minor"),
        ("C#m", "Dbm"),
        ("F# minor", "Gbmin"),
        ("Gbmaj", "F# Major"),
        ("Bb", "A#"),
    ] {
        assert_eq!(canon(a), canon(b), "{a:?} vs {b:?}");
        assert!(!canon(a).is_empty(), "{a:?}");
    }
}

#[test]
fn lowercase_queries_match_index_keys() {
    let keys = |toks: Vec<Token>| -> Vec<String> {
        toks.into_iter()
            .filter(|t| t.kind == TokenKind::Key)
            .map(|t| t.text)
            .collect()
    };
    let mut q = Tokenizer::for_query();
    // (query, index name, shared key token)
    let cases = [
        ("am", "Pad_Amin_90", "key:am"),
        ("am", "Pad A minor", "key:am"),
        ("cm", "Cmin", "key:cm"),
        ("cm", "C minor", "key:cm"),
        ("dbm", "C#m", "key:c#m"),
        ("gbm", "F#m Pad", "key:f#m"),
        ("eb", "D#", "key:d#"),
        ("bb", "Bass Bb", "key:a#"),
        ("fm", "Lead_Fm", "key:fm"),
        ("dark am pad", "Dark_Pad_Am", "key:am"),
    ];
    for (query, name, shared) in cases {
        let qk = keys(q.tokenize(query));
        let ik = keys(tokenize(name));
        assert!(qk.iter().any(|k| k == shared), "query {query:?}: {qk:?}");
        assert!(ik.iter().any(|k| k == shared), "index {name:?}: {ik:?}");
    }
    // The plain word stays at the same position; index mode stays strict.
    assert_eq!(render(&q.tokenize("am")), "key:am|note:a|am");
    assert_eq!(render(&q.tokenize("fm bass")), "key:fm|note:f|fm bass");
    assert_eq!(render(&q.tokenize("a kick")), "a kick");
    assert_eq!(render(&q.tokenize("FM bass")), "fm bass");
    assert_eq!(render(&tokenize("am")), "am");
    assert_eq!(render(&tokenize("fm bass")), "fm bass");
}

#[test]
fn offsets_positions_and_kinds() {
    let toks = tokenize("Bass_Loop 120 BPM_Am");
    let summary: Vec<_> = toks
        .iter()
        .map(|t| (t.text.as_str(), t.kind, t.start, t.end, t.position))
        .collect();
    assert_eq!(
        summary,
        [
            ("bass", TokenKind::Word, 0, 4, 0),
            ("loop", TokenKind::Word, 5, 9, 1),
            ("120bpm", TokenKind::Bpm, 10, 17, 2),
            ("120", TokenKind::Number, 10, 17, 2),
            ("key:am", TokenKind::Key, 18, 20, 3),
            ("note:a", TokenKind::Note, 18, 20, 3),
            ("am", TokenKind::Word, 18, 20, 3),
        ]
    );
}

#[test]
fn config_switches() {
    let mut keep_all = Tokenizer::new(TokenizerConfig {
        stop_words: StopWords::none(),
        drop_padded_numbers: false,
        drop_versions: false,
        query: false,
        singularize: false,
    });
    assert_eq!(
        render(&keep_all.tokenize("Kick_01 v2.wav")),
        "kick 01 v 2 wav"
    );
    assert_eq!(render(&keep_all.tokenize("Kicks")), "kicks");
    // Plural folding (on by default): singular shares the position, also
    // for camelCase catenations; stop-word singulars are not added.
    let mut t = Tokenizer::default();
    assert_eq!(render(&t.tokenize("Kicks/BD")), "kicks|kick bd");
    assert_eq!(render(&t.tokenize("HiHats")), "hi|hihats|hihat hats|hat");
    assert_eq!(render(&t.tokenize("One Shots")), "one shots|shot");
    assert_eq!(render(&t.tokenize("Bass Vox")), "bass vox");
    let mut custom = Tokenizer::new(TokenizerConfig {
        stop_words: StopWords::default().with_extra(["vengeance"]),
        ..TokenizerConfig::default()
    });
    assert_eq!(render(&custom.tokenize("Vengeance Kick")), "kick");
    // Reuse keeps no state between calls.
    assert_eq!(render(&custom.tokenize("Snare")), "snare");
}

#[test]
fn strips_only_media_extensions() {
    let cases = [
        ("Kick.wav", "Kick"),
        ("Kick.Hard.WAV", "Kick.Hard"),
        ("Loop 92.5 BPM", "Loop 92.5 BPM"),
        ("Loop.120", "Loop.120"),
        (".wav", ".wav"),
        ("Pad.aiff", "Pad"),
    ];
    for (name, want) in cases {
        assert_eq!(strip_audio_extension(name), want, "name {name:?}");
    }
}

#[test]
fn odd_input_never_panics_and_spans_are_char_boundaries() {
    let inputs = [
        "#",
        "♯♭#",
        "c♭",
        "'",
        "’Kick’",
        "e\u{301}\u{301}",
        "\u{301}",
        "キック_120bpm",
        "🥁 Kick 🔥",
        "99999999999999999999999 bpm",
        "bpm 0",
        "v",
        "v.1",
        "1.",
        ".5bpm",
        "Ab/",
        "/\\//",
        "Ĳssel ǅemal",
    ];
    for input in inputs {
        for t in tokenize(input) {
            assert!(t.start <= t.end && t.end <= input.len(), "{input:?}");
            assert!(input.is_char_boundary(t.start) && input.is_char_boundary(t.end));
            assert!(!t.text.is_empty(), "{input:?}");
        }
        let _ = filename_prior(std::path::Path::new(input));
    }
}
