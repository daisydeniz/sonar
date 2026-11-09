//! The `fields` legend of `fixtures.json`: one line of documentation (and
//! a unit where the value is a quantity) for every field path a fixture
//! entry can contain. The integration tests assert the legend is complete
//! and has no stale entries.

use std::collections::BTreeMap;

use super::manifest::FieldDoc;

/// The `fields` legend: every dotted path that can appear in a fixture
/// entry (`[]` marks array elements) → description and unit.
pub fn fields_legend() -> BTreeMap<String, FieldDoc> {
    const ROWS: &[(&str, &str, Option<&str>)] = &[
        ("path", "File path relative to the fixture directory, '/'-separated", None),
        ("group", "Fixture group = subdirectory: tonal|chords|loops|oneshots|noise|edge|formats|containers|corrupt", None),
        ("description", "Human-readable description; analysis code must never read it", None),
        ("bytes", "File size", Some("bytes")),
        ("format", "Audio format as written; absent for corrupt files", None),
        ("format.container", "File container: wav | aiff (.aif or .aiff) | flac | mp3 | ogg | m4a", None),
        ("format.codec", "pcm | flac | alac | mp3 | vorbis | aac", None),
        ("format.lossless", "true: decoding must return every written int sample q as q/2^(bits-1) bit-exactly; false (lossy): sample-level truth does not apply, only rate, channels, exact num_frames and the declared level truth (digital silence, except the AAC burst file which declares truth.burst_start_frame)", None),
        ("format.sample_rate", "Sample rate", Some("Hz")),
        ("format.channels", "Interleaved channel count", None),
        ("format.bit_depth", "Bits per sample: 16, 24 or 32; absent for lossy codecs", Some("bits")),
        ("format.sample_format", "int (signed PCM) | float (IEEE 754); absent for lossy codecs", None),
        ("format.format_tag", "WAV fmt tag: pcm (1) | ieee_float (3) | extensible (0xFFFE); WAV only", None),
        ("format.num_frames", "Playable frames; for lossy codecs after a gapless decoder removes encoder delay/padding (LAME tag, Ogg granule positions)", Some("frames")),
        ("format.duration_s", "num_frames / sample_rate", Some("s")),
        ("truth", "Truth values; a field is omitted when it does not apply", None),
        ("truth.kind", "tone (sustained pitched) | one_shot | loop | noise | silence", None),
        ("truth.category", "kick|snare|hat_closed|hat_open|drums|click|synth|sine|noise", None),
        ("truth.bpm", "Loop tempo; loop length is exactly bars*beats_per_bar*60/bpm s (rounded to whole frames)", Some("BPM")),
        ("truth.bars", "Whole bars in the loop", Some("bars")),
        ("truth.beats_per_bar", "Quarter-note beats per bar (time signature numerator)", Some("beats")),
        ("truth.key", "Musical key of the clip", None),
        ("truth.key.tonic", "Tonic, canonical spelling C C# D Eb E F F# G Ab A Bb B", None),
        ("truth.key.tonic_pc", "Tonic pitch class, 0 = C .. 11 = B", None),
        ("truth.key.mode", "major | minor", None),
        ("truth.chords", "Chords in time order", None),
        ("truth.chords[].symbol", "Root + 'm' for minor triads, e.g. F#m, Bb", None),
        ("truth.chords[].root", "Chord root, canonical spelling", None),
        ("truth.chords[].quality", "Triad quality: major | minor", None),
        ("truth.chords[].roman", "Roman numeral in the key (upper case = major triad)", None),
        ("truth.chords[].start_s", "Chord start time", Some("s")),
        ("truth.chords[].duration_s", "Chord length", Some("s")),
        ("truth.note_midi", "MIDI note of the fundamental (60 = C4, 69 = A4)", None),
        ("truth.note_name", "Scientific pitch name, canonical spelling, C4 = MIDI 60", None),
        ("truth.pitch_hz", "Fundamental, 12-TET: 440*2^((note_midi-69)/12)", Some("Hz")),
        ("truth.noise_color", "white | pink", None),
        ("truth.peak_dbfs", "20*log10(max |sample|) of the written samples; int q scaled by 1/2^(bits-1); absent if silent", Some("dBFS")),
        ("truth.rms_dbfs", "20*log10(RMS over all samples and channels); full-scale sine = -3.01; absent if silent", Some("dBFS")),
        ("truth.is_silent", "Every written sample is exactly zero (lossy: the encoded signal is digital silence)", None),
        ("truth.burst_start_frame","Lossy burst files only: frame of the playable stream (after gapless trimming of encoder priming/padding) where the single decoded burst starts; a decoder that trims priming from the start puts the first non-zero sample here (±64 frames for a 1 % threshold on the rising sine window), one that does not is 2112 frames late", Some("frames")),
        ("truth.corrupt", "Present only for deliberately broken files", None),
        ("truth.corrupt.reason", "Machine-readable defect id", None),
        ("truth.corrupt.expect", "error: decoder must return an error | error_or_partial: error or partial/full decode; never panic or hang", None),
    ];
    ROWS.iter()
        .map(|(k, d, u)| {
            let doc = FieldDoc {
                description: (*d).to_owned(),
                unit: u.map(str::to_owned),
            };
            ((*k).to_owned(), doc)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legend_has_units_for_quantities() {
        let l = fields_legend();
        for k in [
            "truth.bpm",
            "truth.pitch_hz",
            "truth.peak_dbfs",
            "format.duration_s",
        ] {
            assert!(l[k].unit.is_some(), "{k}");
        }
    }
}
