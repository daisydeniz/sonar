//! Table-driven filename-prior tests.

use std::path::Path;

use super::*;

/// `"<bpm> <key>"`: `-` for none, `?` suffix for a bare (non-explicit)
/// value, `@dir` suffix for a value from a parent folder.
fn render(p: &FilenamePrior) -> String {
    let suffix = |explicit: bool, source: PriorSource| {
        let bare = if explicit { "" } else { "?" };
        let dir = if source == PriorSource::Directory {
            "@dir"
        } else {
            ""
        };
        format!("{bare}{dir}")
    };
    let bpm = p.bpm.map_or_else(
        || "-".to_owned(),
        |b| format!("{}{}", b.bpm, suffix(b.explicit, b.source)),
    );
    let key = p.key.map_or_else(
        || "-".to_owned(),
        |k| format!("{}{}", k.key, suffix(k.explicit, k.source)),
    );
    format!("{bpm} {key}")
}

#[test]
fn filename_prior_table() {
    let cases: &[(&str, &str)] = &[
        // Positives.
        ("Bass_Loop_128_Am.wav", "128? Am"),
        ("Loop 120bpm.wav", "120 -"),
        ("BPM_140_Trap_Loop.wav", "140 -"),
        ("Loop 92.5 BPM.wav", "92.5 -"),
        ("Pad_F#_minor_90bpm.wav", "90 F#m"),
        ("Gbmaj_Chords.wav", "- F#maj"),
        ("Synth_Ebmin.wav", "- D#m"),
        ("Lead - Bb.wav", "- A#"),
        ("FM_Bass_C#m.wav", "- C#m"),
        ("Kick 128 Am.wav", "128? Am"),
        ("Bass_Loop_120_C.wav", "120? C?"),
        ("Am_A_minor.wav", "- Am"),
        ("Am_to_C_Loop.wav", "- Am"),
        ("Vocal Chop E 120bpm.wav", "120 -"),
        // Negatives: numbers that are not tempos.
        ("Kick 808.wav", "- -"),
        ("TR808 Loop 909.wav", "- -"),
        ("Kick_44100.wav", "- -"),
        ("Kick 24bit.wav", "- -"),
        ("Kick_01.wav", "- -"),
        ("Kick 75.wav", "- -"),
        ("Snare 120.wav", "- -"),
        ("Loop 30bpm.wav", "- -"),
        ("Loop A 120.wav", "120? -"),
        // Negatives: letters/words that are not keys.
        ("A Kick.wav", "- -"),
        ("FM Bass.wav", "- -"),
        ("EBM Kick -6dB.wav", "- -"),
        // Ambiguous: conflicting values give nothing.
        ("Loop 120 128.wav", "- -"),
        ("Loop 120bpm 140bpm.wav", "- -"),
        ("Bass_Am_Cm_120bpm.wav", "120 -"),
        // Parent folders: explicit values only, file name wins.
        ("Loops/124 BPM/Am/Kick 3.wav", "124@dir Am@dir"),
        ("Loops/128/Kick.wav", "- -"),
        ("C/Kick.wav", "- -"),
        ("124bpm/House_Loop_Am_126.wav", "126? Am"),
        ("Packs/90 BPM/Keys/Rhodes Loop.wav", "90@dir -"),
        ("90 BPM/Pack/Keys/Rhodes Loop.wav", "- -"), // beyond PRIOR_DIR_LEVELS
        ("", "- -"),
        ("/", "- -"),
    ];
    for (path, want) in cases {
        assert_eq!(
            render(&filename_prior(Path::new(path))),
            *want,
            "path {path:?}"
        );
    }
}

/// T10.1: bracketed keys, trailing note letters in tonal names, lowercase
/// keys with tempo context — and the names that must stay without a key.
#[test]
fn filename_key_context_table() {
    let cases: &[(&str, &str)] = &[
        // Bracketed keys.
        ("ABTSRR2 - Tom Loop 08 (C)(120 BPM).wav", "120 C"),
        ("Bass Loop [F#] 128bpm.wav", "128 F#"),
        ("Pad (Am).wav", "- Am"),
        ("Vocal Loop (E) 90bpm.wav", "90 E"),
        ("Sub [G].wav", "- G"),
        ("Rhodes Chords (D)(85 BPM).aif", "85 D"),
        // Trailing note letter in a tonal one-shot name.
        ("Piano_C.wav", "- C?"),
        ("Sub Bass C.wav", "- C?"),
        ("808_Long_02_G.wav", "- G?"),
        ("Pluck - E.wav", "- E?"),
        ("Dunun Low F.wav", "- F?"),
        ("Surdo 3 A.wav", "- A?"),
        ("Marimba_Hit_B", "- B?"),
        ("SubBass_D.wav", "- D?"),
        // Lowercase keys with a BPM or loop word.
        ("bass_loop_120_am.wav", "120? Am?"),
        ("synth loop 128bpm gm.wav", "128 Gm?"),
        ("keys_loop_c#m.wav", "- C#m"),
        ("pad_90bpm_eb.wav", "90 D#?"),
        ("arp loop f#.wav", "- F#"),
        ("loop_120_fm.wav", "120? Fm?"),
        // Unchanged / negatives.
        ("Fm_Bass.wav", "- Fm"),
        ("Fm_Bass_Growl.wav", "- Fm"),
        ("Kick -6dB.wav", "- -"),
        ("Bass Loop -6 db 120bpm.wav", "120 -"),
        ("fm_bass_loop_120.wav", "120? -"),
        ("ebm kick loop 120bpm.wav", "120 -"),
        ("am bass.wav", "- -"),
        ("gm synth.wav", "- -"),
        ("Kick B.wav", "- -"),
        ("Snare E.wav", "- -"),
        ("Vol A.wav", "- -"),
        ("Take B.wav", "- -"),
        ("Bass Take B.wav", "- -"),
        ("Synth Vol A.wav", "- -"),
        ("Kick (A).wav", "- -"),
        ("Kick Loop (A) 120bpm.wav", "120 -"),
        ("Bass E Long.wav", "- -"),
        ("a bass e.wav", "- -"),
        ("Bass_C_01.wav", "- -"),
        ("Lead C2.wav", "- -"),
        ("Pad Am (C).wav", "- Am"),
        ("Bass (C)/Kick.wav", "- C@dir"),
    ];
    for (path, want) in cases {
        assert_eq!(
            render(&filename_prior(Path::new(path))),
            *want,
            "path {path:?}"
        );
    }
}

#[test]
fn prior_matches_tokens() {
    // The prior's BPM/key equal the tokens the index stores for the name.
    let name = "Deep_Loop_124_Ebm";
    let p = filename_prior(Path::new(name));
    let toks: Vec<_> = tokenize(name).into_iter().map(|t| t.text).collect();
    let bpm = p.bpm.map(|b| format!("{}bpm", b.bpm));
    let key = p.key.map(|k| k.key.token());
    assert_eq!(bpm.as_deref(), Some("124bpm"));
    assert_eq!(key.as_deref(), Some("key:d#m"));
    assert!(toks.iter().any(|t| Some(t) == bpm.as_ref()));
    assert!(toks.iter().any(|t| Some(t) == key.as_ref()));
}
