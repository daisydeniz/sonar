//! The `containers` group: every non-WAV extension the scanner accepts
//! (`aif aiff flac m4a mp3 ogg`), a few rate/channel/bit-depth
//! combinations each.
//!
//! Lossless files (AIFF, FLAC, ALAC) carry real content with full truth
//! and must decode bit-exactly; the content is chosen to exercise each
//! encoder's code paths (FLAC CONSTANT/VERBATIM/FIXED subframes, ALAC full
//! and partial frames). Lossy files (MP3, Ogg Vorbis, AAC-LC) are digital
//! silence: they verify container parsing, rate/channels and exact gapless
//! length (encoder delay/padding, Ogg end trim, AAC `elst`/`iTunSMPB`), not
//! sample values. One AAC file adds a single burst at a known frame to
//! prove priming is trimmed from the start.

use super::aac::{AacSpec, Tags};
use super::drums::Pattern;
use super::manifest::{Group, Kind, NoiseColor};
use super::spec::{Body, Builder, Content, LossyCodec, Store};
use super::wav::{SampleFormat, WavSpec};

use SampleFormat::{Int16, Int24};

/// Lossy streams: `(codec, rate Hz, channels, frames, description)`.
const LOSSY: [(LossyCodec, u32, u16, u64, &str); 7] = [
    (LossyCodec::Mp3, 44_100, 2, 44_100, "MPEG-1 Layer III"),
    (LossyCodec::Mp3, 48_000, 1, 30_000, "MPEG-1 Layer III"),
    (LossyCodec::Mp3, 22_050, 1, 11_025, "MPEG-2 LSF Layer III"),
    (
        LossyCodec::Mp3,
        32_000,
        2,
        1_000,
        "MPEG-1 Layer III, shorter than 2 frames",
    ),
    (LossyCodec::Vorbis, 44_100, 2, 44_100, "Ogg Vorbis"),
    (
        LossyCodec::Vorbis,
        48_000,
        1,
        12_345,
        "Ogg Vorbis, end-trimmed last packet",
    ),
    (
        LossyCodec::Vorbis,
        22_050,
        2,
        5_000,
        "Ogg Vorbis, single audio page",
    ),
];

fn a4(secs: f64) -> Content {
    Content::Sine {
        midi: 69,
        secs,
        kind: Kind::Tone,
    }
}

/// Appends the `containers` group to `b`.
pub(super) fn containers(b: &mut Builder) {
    b.group(Group::Containers, "clip");
    lossless(b);
    for (codec, sample_rate, channels, frames, what) in LOSSY {
        let ext = match codec {
            LossyCodec::Mp3 => "mp3",
            LossyCodec::Vorbis => "ogg",
        };
        let secs = frames as f64 / f64::from(sample_rate);
        let desc = format!("digital silence {secs:.3} s, {what}, {sample_rate} Hz, {channels} ch");
        let body = Body::LossySilence {
            codec,
            sample_rate,
            channels,
            frames,
        };
        b.push_ext(body, desc, ext);
    }
    aac(b);
}

/// AAC-LC in `.m4a` with encoder priming (2112 frames). Declared lengths
/// are not multiples of 1024, so the end padding is non-trivial.
fn aac(b: &mut Builder) {
    let spec = |sample_rate, channels, frames, tags, burst_unit| AacSpec {
        sample_rate,
        channels,
        frames,
        tags,
        burst_unit,
    };
    let cases = [
        (
            spec(44_100, 2, 44_100, Tags::Both, None),
            "digital silence 1 s, AAC-LC 44.1 kHz stereo, priming 2112 via elst + iTunSMPB",
        ),
        (
            spec(48_000, 1, 12_345, Tags::ElstOnly, None),
            "digital silence 0.257 s, AAC-LC 48 kHz mono, priming 2112 via elst only",
        ),
        (
            spec(32_000, 1, 20_000, Tags::SmpbOnly, None),
            "digital silence 0.625 s, AAC-LC 32 kHz mono, priming 2112 via iTunSMPB only",
        ),
        (
            spec(44_100, 1, 20_000, Tags::Both, Some(5)),
            "silence with one 355 Hz AAC window starting at frame 3008 of the \
             playable stream (access unit 5 minus 2112 priming), AAC-LC 44.1 kHz mono",
        ),
    ];
    for (spec, desc) in cases {
        b.push_ext(Body::Aac(spec), desc.to_owned(), "m4a");
    }
}

fn lossless(b: &mut Builder) {
    // AIFF: one `.aif` and one `.aiff`.
    let body = Body::Audio {
        spec: WavSpec::new(44_100, 2, Int16),
        content: a4(1.0),
        peak_dbfs: Some(-6.0),
        store: Store::Aiff,
    };
    b.push_ext(body, "A4 sine 1 s, AIFF 16-bit stereo".into(), "aif");
    let pluck = Content::Pluck {
        midi: 60,
        secs: 1.5,
    };
    let s = WavSpec::new(48_000, 1, Int24);
    b.add_stored(
        Store::Aiff,
        s,
        pluck,
        Some(-3.0),
        "C4 pluck 1.5 s, AIFF 24-bit mono",
    );

    // FLAC.
    let lp = Content::DrumLoop {
        bpm: 128.0,
        bars: 1,
        pattern: Pattern::FourOnFloor,
    };
    let s = WavSpec::new(44_100, 2, Int16);
    let d = "FourOnFloor drum loop, 128 BPM, 1 bar of 4/4, FLAC 16-bit stereo";
    b.add_stored(Store::Flac, s, lp, Some(-1.0), d);
    let noise = Content::Noise {
        color: NoiseColor::White,
        secs: 0.5,
        burst: None,
    };
    let s = WavSpec::new(48_000, 1, Int24);
    b.add_stored(
        Store::Flac,
        s,
        noise,
        Some(-6.0),
        "white noise 0.5 s, FLAC 24-bit mono",
    );
    let s = WavSpec::new(11_025, 1, Int16);
    let d = "A4 sine 1 s, FLAC 16-bit mono at 11025 Hz (rate only in STREAMINFO)";
    b.add_stored(Store::Flac, s, a4(1.0), Some(-6.0), d);
    let s = WavSpec::new(96_000, 2, Int24);
    let silence = Content::Silence { frames: 9_000 };
    b.add_stored(
        Store::Flac,
        s,
        silence,
        None,
        "digital silence, FLAC 24-bit stereo 96 kHz",
    );

    // ALAC in M4A.
    let s = WavSpec::new(44_100, 2, Int16);
    b.add_stored(
        Store::Alac,
        s,
        a4(1.0),
        Some(-6.0),
        "A4 sine 1 s, ALAC 16-bit stereo",
    );
    let pluck = Content::Pluck {
        midi: 64,
        secs: 1.0,
    };
    let s = WavSpec::new(48_000, 1, Int24);
    b.add_stored(
        Store::Alac,
        s,
        pluck,
        Some(-3.0),
        "E4 pluck 1 s, ALAC 24-bit mono",
    );
    let noise = Content::Noise {
        color: NoiseColor::Pink,
        secs: 8_192.0 / 44_100.0,
        burst: None,
    };
    let s = WavSpec::new(44_100, 1, Int16);
    let d = "pink noise, exactly 2 full ALAC frames (8192), 16-bit mono";
    b.add_stored(Store::Alac, s, noise, Some(-6.0), d);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn every_scanner_extension_is_covered() {
        let mut b = Builder::new();
        containers(&mut b);
        let exts: BTreeSet<&str> = b
            .out
            .iter()
            .filter_map(|f| f.path.rsplit_once('.').map(|(_, e)| e))
            .collect();
        // Every scanner extension except `wav` (all other groups are WAV).
        let want: BTreeSet<&str> = crate::scan::AUDIO_EXTENSIONS
            .iter()
            .copied()
            .filter(|e| *e != "wav")
            .collect();
        assert_eq!(exts, want);
        assert!(b.out.iter().all(|f| f.group == Group::Containers));
    }
}
