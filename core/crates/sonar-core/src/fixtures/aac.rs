//! Minimal AAC-LC in M4A writer: hand-written raw access units with real
//! encoder priming, the most common `.m4a` an Apple or ffmpeg encoder
//! produces.
//!
//! Bitstream (ISO/IEC 14496-3 Subpart 4: §4.4.2 syntax of
//! `single_channel_element`, `channel_pair_element`,
//! `individual_channel_stream`, `ics_info`, `section_data`,
//! `scale_factor_data`; §4.5.4 scale-factor bands; §4.6.3 spectral data;
//! Annex 4.A Huffman codebooks): every access unit is one
//! `single_channel_element` or `channel_pair_element` followed by `ID_END`,
//! coded with `ONLY_LONG_SEQUENCE`, the sine window and 1,024 samples per
//! channel. Two kinds of frame exist:
//!
//! - **silent**: `max_sfb = 0`, so no section, scale-factor or spectral data
//!   is coded and the spectrum is all zero (stereo: `common_window = 0`, two
//!   independent channel streams);
//! - **burst** (mono, 44.1/48 kHz only): `max_sfb = 5`, bands 0-3 in the
//!   zero codebook, band 4 (bins 16..20 in the 44.1/48 kHz long-window
//!   band table) in spectral codebook 1 (signed 4-tuples). One scale factor
//!   (delta 0 = the 1-bit scale-factor codeword `0`, so `scale_factor =
//!   global_gain`) and one quad `(1, 0, 0, 0)` = codebook-1 index 67 = the
//!   5-bit code `0x10`. The result is a single MDCT coefficient (bin 16,
//!   ≈ 355 Hz at 44.1 kHz): a 2,048-sample half-sine-windowed cosine that
//!   starts exactly at the first output sample of its access unit.
//!
//! Container (ISO/IEC 14496-12, 14496-14): `mp4a` sample entry with an
//! `esds` holding the `AudioSpecificConfig` (AOT 2 = AAC-LC), then the
//! priming metadata of real encoders, in either or both of two forms:
//!
//! - an edit list `edts/elst` with one edit: `media_time` =
//!   [`PRIMING`] and `segment_duration` = the declared length (movie
//!   timescale = media timescale = sample rate);
//! - Apple's `iTunSMPB` freeform tag (`moov/udta/meta/ilst/----` with
//!   `mean` = `com.apple.iTunes`, `name` = `iTunSMPB`): hex fields
//!   `0`, priming, padding, valid length.
//!
//! The access-unit count is `ceil((PRIMING + frames) / 1024)`, so the
//! stream is `PRIMING` frames longer at the start and `padding` frames at
//! the end than the declared length. [`PRIMING`] = 2,112 is Apple's value
//! (1,024 codec delay + 1,088 encoder lookahead).

use super::bits::BitWriter;
use super::mp4box::{audio_stsd, boxed, file, full_box, stts, u32s, Track};

/// Samples per channel in one AAC access unit.
pub const FRAME_LEN: usize = 1024;
/// Encoder priming (frames to discard from the start of the decoded stream).
pub const PRIMING: u64 = 2112;
/// `global_gain` of the burst frame; sets its amplitude (measured, see the
/// decode tests).
const BURST_GAIN: u64 = 192;

/// Which priming metadata the file carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tags {
    /// `elst` and `iTunSMPB` (iTunes, recent ffmpeg).
    Both,
    /// `elst` only (typical ffmpeg/MP4Box output).
    ElstOnly,
    /// `iTunSMPB` only.
    SmpbOnly,
}

/// One AAC-LC fixture.
#[derive(Debug, Clone, PartialEq)]
pub struct AacSpec {
    /// Sample rate (Hz); one of the 12 AAC rates.
    pub sample_rate: u32,
    /// 1 or 2.
    pub channels: u16,
    /// Declared playable frames (after trimming priming and padding).
    pub frames: u64,
    /// Priming metadata written.
    pub tags: Tags,
    /// Access unit (0-based, in the *decoded* stream) holding the burst
    /// instead of silence; mono 44.1/48 kHz only.
    pub burst_unit: Option<usize>,
}

impl AacSpec {
    /// Access units in the file.
    pub fn units(&self) -> usize {
        (PRIMING + self.frames).div_ceil(FRAME_LEN as u64) as usize
    }

    /// Frames of end padding.
    pub fn padding(&self) -> u64 {
        self.units() as u64 * FRAME_LEN as u64 - PRIMING - self.frames
    }

    /// Frame of the playable stream at which the burst's window starts
    /// (its first output sample); `None` without a burst.
    pub fn burst_start_frame(&self) -> Option<u64> {
        self.burst_unit
            .map(|u| ((u * FRAME_LEN) as u64).saturating_sub(PRIMING))
    }
}

/// Writes the `.m4a`; `None` when the rate/channels/burst combination is
/// not representable.
pub fn encode(spec: &AacSpec) -> Option<Vec<u8>> {
    let rate_index = rate_index(spec.sample_rate)?;
    if !(1..=2).contains(&spec.channels) {
        return None;
    }
    let units = spec.units();
    let frames = u32::try_from(spec.frames).ok()?;
    if let Some(u) = spec.burst_unit {
        let ok = spec.channels == 1 && matches!(spec.sample_rate, 44_100 | 48_000);
        // The burst must start in the playable stream and fit before the end.
        if !ok || (u * FRAME_LEN) as u64 <= PRIMING || u + 2 > units {
            return None;
        }
    }
    let access_units: Vec<Vec<u8>> = (0..units)
        .map(|i| {
            if spec.burst_unit == Some(i) {
                burst_unit()
            } else {
                silent_unit(spec.channels)
            }
        })
        .collect();
    let max_bytes = access_units.iter().map(Vec::len).max().unwrap_or(0) as u32;
    let total: usize = access_units.iter().map(Vec::len).sum();
    let media_frames = (units * FRAME_LEN) as u32;
    let avg_bit_rate =
        (total as u64 * 8 * u64::from(spec.sample_rate) / u64::from(media_frames)) as u32;
    let mut track = Track {
        sample_rate: spec.sample_rate,
        media_frames,
        movie_frames: frames,
        stsd: audio_stsd(
            b"mp4a",
            spec.channels,
            16,
            spec.sample_rate,
            &esds(rate_index, spec.channels, max_bytes, avg_bit_rate),
        ),
        stts: stts(units, FRAME_LEN as u32, FRAME_LEN as u32),
        edts: Vec::new(),
        udta: Vec::new(),
    };
    if spec.tags != Tags::SmpbOnly {
        // Without an edit the movie is the whole media.
        track.edts = edts(frames);
    }
    if spec.tags == Tags::SmpbOnly {
        track.movie_frames = media_frames;
    }
    if spec.tags != Tags::ElstOnly {
        track.udta = udta(spec.padding(), spec.frames);
    }
    Some(file(&access_units, &track))
}

/// Sampling-frequency index (ISO/IEC 14496-3 Table 1.18).
fn rate_index(rate: u32) -> Option<u8> {
    const RATES: [u32; 12] = [
        96_000, 88_200, 64_000, 48_000, 44_100, 32_000, 24_000, 22_050, 16_000, 12_000, 11_025,
        8_000,
    ];
    RATES.iter().position(|&r| r == rate).map(|i| i as u8)
}

const ID_SCE: u64 = 0;
const ID_CPE: u64 = 1;
const ID_END: u64 = 7;

/// `ics_info()` for `ONLY_LONG_SEQUENCE`, sine window, no prediction.
fn ics_info(w: &mut BitWriter, max_sfb: u64) {
    w.put(0, 1); // ics_reserved_bit
    w.put(0, 2); // window_sequence = ONLY_LONG_SEQUENCE
    w.put(0, 1); // window_shape = sine
    w.put(max_sfb, 6);
    w.put(0, 1); // predictor_data_present
}

/// The three "tool present" flags before the spectral data: pulse, TNS,
/// gain control.
fn no_tools(w: &mut BitWriter) {
    w.put(0, 3);
}

/// An access unit of digital silence.
fn silent_unit(channels: u16) -> Vec<u8> {
    let mut w = BitWriter::new();
    let silent_ics = |w: &mut BitWriter| {
        w.put(0, 8); // global_gain
        ics_info(w, 0);
        no_tools(w);
    };
    if channels == 1 {
        w.put(ID_SCE, 3);
        w.put(0, 4); // element_instance_tag
        silent_ics(&mut w);
    } else {
        w.put(ID_CPE, 3);
        w.put(0, 4);
        w.put(0, 1); // common_window
        silent_ics(&mut w);
        silent_ics(&mut w);
    }
    w.put(ID_END, 3);
    w.into_bytes()
}

/// A mono access unit with one non-zero MDCT coefficient (bin 16).
fn burst_unit() -> Vec<u8> {
    let mut w = BitWriter::new();
    w.put(ID_SCE, 3);
    w.put(0, 4);
    w.put(BURST_GAIN, 8); // global_gain
    ics_info(&mut w, 5);
    // section_data (long window: 5-bit lengths): bands 0..4 codebook 0
    // (zero), band 4 codebook 1.
    w.put(0, 4);
    w.put(4, 5);
    w.put(1, 4);
    w.put(1, 5);
    // scale_factor_data: band 4 only; delta 0 is the 1-bit code `0`.
    w.put(0, 1);
    no_tools(&mut w);
    // spectral_data of band 4 (bins 16..20): one quad, codebook 1 index
    // 27·(1+1) + 9·(0+1) + 3·(0+1) + (0+1) = 67, a 5-bit code.
    w.put(0x10, 5);
    w.put(ID_END, 3);
    w.into_bytes()
}

/// `esds` (ISO/IEC 14496-1 §7.2.6): ES, decoder-config and
/// `AudioSpecificConfig` descriptors, each with a 1-byte length.
fn esds(rate_index: u8, channels: u16, max_bytes: u32, avg_bit_rate: u32) -> Vec<u8> {
    let mut asc = BitWriter::new();
    asc.put(2, 5); // audioObjectType = AAC LC
    asc.put(u64::from(rate_index), 4);
    asc.put(u64::from(channels), 4); // channelConfiguration 1 = mono, 2 = stereo
    asc.put(0, 3); // frameLengthFlag 0 (1024), dependsOnCoreCoder, extensionFlag
    let asc = descriptor(0x05, &asc.into_bytes());
    let config = descriptor(
        0x04,
        &[
            &[0x40, 0x15][..], // MPEG-4 audio, audio stream
            &[0, 0, 0],        // buffer size
            &max_bytes.to_be_bytes(),
            &avg_bit_rate.to_be_bytes(),
            &asc,
        ]
        .concat(),
    );
    let sl = descriptor(0x06, &[0x02]);
    let es = descriptor(0x03, &[&[0, 0, 0][..], &config, &sl].concat()); // ES_ID 0, no flags
    full_box(b"esds", 0, &es)
}

/// An MPEG-4 descriptor with a one-byte (< 128) length.
fn descriptor(tag: u8, body: &[u8]) -> Vec<u8> {
    debug_assert!(body.len() < 128);
    [&[tag, body.len() as u8][..], body].concat()
}

/// `edts` with one edit: skip [`PRIMING`] media frames, play `frames`.
fn edts(frames: u32) -> Vec<u8> {
    let elst = full_box(b"elst", 0, &u32s(&[1, frames, PRIMING as u32, 0x0001_0000]));
    boxed(b"edts", &elst)
}

/// `udta/meta/ilst/----` carrying `iTunSMPB`.
fn udta(padding: u64, frames: u64) -> Vec<u8> {
    let text = format!(
        " 00000000 {PRIMING:08X} {padding:08X} {frames:016X} 00000000 00000000 00000000 \
         00000000 00000000 00000000 00000000"
    );
    let mean = full_box(b"mean", 0, b"com.apple.iTunes");
    let name = full_box(b"name", 0, b"iTunSMPB");
    let data = full_box(b"data", 1, &[&[0; 4][..], text.as_bytes()].concat()); // UTF-8, locale 0
    let item = boxed(b"----", &[mean, name, data].concat());
    let hdlr = full_box(
        b"hdlr",
        0,
        &[&[0; 4][..], b"mdir", b"appl", &[0; 9]].concat(),
    );
    let meta = full_box(b"meta", 0, &[hdlr, boxed(b"ilst", &item)].concat());
    boxed(b"udta", &meta)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(tags: Tags, burst_unit: Option<usize>) -> AacSpec {
        AacSpec {
            sample_rate: 44_100,
            channels: 1,
            frames: 20_000,
            tags,
            burst_unit,
        }
    }

    #[test]
    fn unit_count_and_padding_follow_priming() {
        let s = spec(Tags::Both, None);
        // 2,112 + 20,000 = 22,112 → 22 units = 22,528 frames.
        assert_eq!(s.units(), 22);
        assert_eq!(s.padding(), 22 * 1024 - 22_112);
        assert_ne!(s.frames % 1024, 0, "padding must be non-trivial");
    }

    #[test]
    fn silent_mono_unit_is_all_zero_bits_then_end() {
        // SCE tag(3)+instance(4)+gain(8)+ics_info(11)+tools(3) = 29 zero
        // bits, then ID_END = 0b111.
        assert_eq!(silent_unit(1), [0, 0, 0, 0x07]);
        // CPE: id(3)=001, instance(4), common_window(1), two channel streams
        // of 8+11+3 = 22 zero bits, END: 3+4+1+22+22+3 = 55 bits.
        let st = silent_unit(2);
        assert_eq!(st.len(), 7);
        assert_eq!(st[0], 0b0010_0000);
        assert_eq!(st[6], 0b0000_1110, "END after 52 bits");
    }

    #[test]
    fn burst_unit_bit_layout() {
        // id(3)=0 tag(4)=0 | gain(8)=192 | ics_info: 0 00 0 000101 0 |
        // cb 0000 len 00100 | cb 0001 len 00001 | sf 0 | tools 000 |
        // quad 10000 | END 111
        let mut want = BitWriter::new();
        for (v, n) in [
            (0u64, 3u32),
            (0, 4),
            (192, 8),
            (0, 1),
            (0, 2),
            (0, 1),
            (5, 6),
            (0, 1),
            (0b0000_00100, 9),
            (0b0001_00001, 9),
            (0, 1),
            (0, 3),
            (0b10000, 5),
            (0b111, 3),
        ] {
            want.put(v, n);
        }
        assert_eq!(burst_unit(), want.into_bytes());
    }

    #[test]
    fn esds_carries_the_audio_specific_config() {
        let e = esds(4, 1, 100, 64_000);
        // ASC for AAC-LC 44.1 kHz mono: 00010 0100 0001 000 = 0x1208.
        let pos = e.windows(2).position(|w| w == [0x12, 0x08]).unwrap();
        assert_eq!(
            &e[pos - 2..pos],
            &[0x05, 0x02],
            "DecoderSpecificInfo tag, len"
        );
        // Box header (8) + version/flags (4), then the ES_Descriptor.
        assert_eq!(e[12], 0x03, "ES_Descriptor tag");
        assert_eq!(
            e.len(),
            14 + usize::from(e[13]),
            "ES length covers the rest"
        );
    }

    #[test]
    fn tags_select_the_metadata_boxes() {
        let has = |b: &[u8], k: &[u8]| b.windows(k.len()).any(|w| w == k);
        for (tags, elst, smpb) in [
            (Tags::Both, true, true),
            (Tags::ElstOnly, true, false),
            (Tags::SmpbOnly, false, true),
        ] {
            let b = encode(&spec(tags, None)).unwrap();
            assert_eq!(has(&b, b"elst"), elst, "{tags:?}");
            assert_eq!(has(&b, b"iTunSMPB"), smpb, "{tags:?}");
        }
        let b = encode(&spec(Tags::Both, None)).unwrap();
        assert!(has(&b, b" 00000000 00000840 000001A0 0000000000004E20 "));
    }

    #[test]
    fn unrepresentable_specs_are_rejected() {
        let mut s = spec(Tags::Both, None);
        s.sample_rate = 44_000;
        assert!(encode(&s).is_none());
        let mut s = spec(Tags::Both, None);
        s.channels = 3;
        assert!(encode(&s).is_none());
        // Burst in the priming (would be trimmed away), stereo, last unit.
        assert!(encode(&spec(Tags::Both, Some(2))).is_none());
        let mut s = spec(Tags::Both, Some(5));
        s.channels = 2;
        assert!(encode(&s).is_none());
        assert!(encode(&spec(Tags::Both, Some(21))).is_none());
        assert_eq!(spec(Tags::Both, Some(5)).burst_start_frame(), Some(3_008));
    }
}
