//! Opening a file with symphonia: probe the container, pick the audio
//! track, validate its parameters against hard limits and build the codec.
//!
//! Every symphonia error is mapped to a [`crate::Error`] value here; the
//! limits below reject absurd headers *before* anything sized by them is
//! allocated (channel count × packet size drives decoder buffer sizes).

use std::fs::File;
use std::io;
use std::path::Path;

use symphonia::core::audio::Channels;
use symphonia::core::codecs::audio::well_known as codec_ids;
use symphonia::core::codecs::audio::{AudioCodecId, AudioDecoder, AudioDecoderOptions};
use symphonia::core::codecs::CodecParameters;
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::well_known::{
    FORMAT_ID_ISOMP4, FORMAT_ID_MP1, FORMAT_ID_MP2, FORMAT_ID_MP3,
};
use symphonia::core::formats::{FormatOptions, FormatReader, TrackType};
use symphonia::core::io::{MediaSource, MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::TimeBase;

use super::gapless::{self, Gapless};
use crate::{Error, Result};

/// Lowest accepted source sample rate (Hz).
pub const MIN_SAMPLE_RATE: u32 = 1_000;
/// Highest accepted source sample rate (Hz); 768 kHz is the highest rate
/// any shipping converter/DAW supports.
pub const MAX_SAMPLE_RATE: u32 = 768_000;
/// Highest accepted channel count (beyond 7.1.4 / third-order ambisonics).
pub const MAX_CHANNELS: usize = 32;
/// Highest accepted frames per packet declared by the container (FLAC
/// allows 65,535; PCM WAV packets are 1,152). Bounds decoder buffers to
/// `MAX_FRAMES_PER_PACKET × MAX_CHANNELS × 8 B` = 64 MiB worst case.
pub const MAX_FRAMES_PER_PACKET: u64 = 1 << 18;

/// A probed file ready to stream packets.
pub(super) struct Opened {
    /// Demuxer positioned at the first packet.
    pub format: Box<dyn FormatReader>,
    /// Decoder for the selected track.
    pub decoder: Box<dyn AudioDecoder>,
    /// Selected track id; packets of other tracks are skipped.
    pub track_id: u32,
    /// Track time base (packet durations are in these units).
    pub time_base: Option<TimeBase>,
    /// Source sample rate (Hz), validated.
    pub sample_rate: u32,
    /// Source channel count, validated (1..=[`MAX_CHANNELS`]).
    pub channels: usize,
    /// Bits per sample for PCM-like codecs.
    pub bit_depth: Option<u32>,
    /// Channel layout declared by the container (plane order of decoded
    /// buffers), if any; drives the BS.1770 channel weights.
    pub layout: Option<Channels>,
    /// Codec short name, e.g. `pcm_s16le`, `flac`, `mp3`.
    pub codec: String,
    /// Codec id of the selected track (drives the header length path).
    pub codec_id: AudioCodecId,
    /// Container short name, e.g. `wave`, `aiff`, `flac`.
    pub container: String,
    /// Playable frames declared by the container, if any.
    pub declared_frames: Option<u64>,
    /// The demuxer reports `Packet::dur` *before* gapless trimming
    /// (symphonia 0.6.1 MPEG audio; see [`super::stream::valid_ticks`]).
    pub dur_includes_trim: bool,
    /// Encoder priming/padding of an MP4 file (symphonia ignores it, see
    /// [`super::gapless`]); [`Opened::declared_frames`] already reflects it.
    pub gapless: Option<Gapless>,
}

/// Opens and probes `path`. Errors: [`Error::Io`] if the file cannot be
/// opened or read, [`Error::UnsupportedFormat`] for unknown containers or
/// codecs, [`Error::Decode`] for damaged headers or out-of-limit values.
pub(super) fn open(path: &Path) -> Result<Opened> {
    let file = File::open(path).map_err(|e| Error::io(path, e))?;
    let mut src = open_source(path, Box::new(file))?;
    if src.format.format_info().format == FORMAT_ID_ISOMP4 {
        if let Some(g) = gapless::read_isomp4(path, src.sample_rate, src.track_id) {
            // The demuxer counts every sample of the media; the playable
            // length is what is left after the edit.
            src.declared_frames = src.declared_frames.map(|raw| {
                let after = raw.saturating_sub(g.start);
                g.len.map_or(after, |l| after.min(l))
            });
            src.gapless = Some(g);
        }
    }
    Ok(src)
}

/// [`open`] on an arbitrary byte source; `path` names it in errors and
/// provides the extension hint. Real read failures (anything but
/// end-of-file) surface as [`Error::Io`].
pub(super) fn open_source(path: &Path, source: Box<dyn MediaSource>) -> Result<Opened> {
    let mss = MediaSourceStream::new(source, MediaSourceStreamOptions::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let format = symphonia::default::get_probe()
        .probe(
            &hint,
            mss,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|e| map_error(path, e))?;

    let track = format
        .default_track(TrackType::Audio)
        .ok_or_else(|| unsupported(path, "no decodable audio track"))?;
    let track_id = track.id;
    let time_base = track.time_base;
    let declared_frames = track.num_frames;
    let Some(CodecParameters::Audio(params)) = &track.codec_params else {
        return Err(unsupported(path, "no audio codec parameters"));
    };

    let sample_rate = params
        .sample_rate
        .ok_or_else(|| decode_err(path, "sample rate missing"))?;
    if !(MIN_SAMPLE_RATE..=MAX_SAMPLE_RATE).contains(&sample_rate) {
        return Err(decode_err(
            path,
            format!(
                "sample rate {sample_rate} Hz outside {MIN_SAMPLE_RATE}..={MAX_SAMPLE_RATE} Hz"
            ),
        ));
    }
    let layout = params.channels.clone();
    let channels = layout.as_ref().map_or(0, Channels::count);
    if !(1..=MAX_CHANNELS).contains(&channels) {
        return Err(decode_err(
            path,
            format!("channel count {channels} outside 1..={MAX_CHANNELS}"),
        ));
    }
    // `None` is safe: every enabled decoder bounds its buffer without it
    // (symphonia 0.6.1): PCM starts at capacity 0 and grows to the packet
    // the demuxer delivered (codec-pcm lib.rs:245); ADPCM rejects `None`
    // (codec-adpcm lib.rs:83); FLAC sizes from the u16 STREAMINFO max block
    // (bundle-flac decoder.rs:115-123); ALAC caps the cookie frame length at
    // 16 × 4096 (codec-alac lib.rs:298); Vorbis uses the bounds-checked
    // blocksize ≤ 8192 (codec-vorbis lib.rs:127, 462-466); AAC is fixed at
    // 1024 (codec-aac aac/mod.rs:94-106); MP3 at 1152 (bundle-mp3
    // decoder.rs:97).
    if let Some(n) = params.max_frames_per_packet {
        if n > MAX_FRAMES_PER_PACKET {
            return Err(decode_err(
                path,
                format!("{n} frames per packet exceeds {MAX_FRAMES_PER_PACKET}"),
            ));
        }
    }
    let bit_depth = params
        .bits_per_sample
        .or_else(|| float_bits(params.codec))
        .or_else(|| alac_bits(params.codec, params.extra_data.as_deref()));

    let decoder = symphonia::default::get_codecs()
        .make_audio_decoder(params, &AudioDecoderOptions::default())
        .map_err(|e| map_error(path, e))?;
    let codec_id = params.codec;
    let codec = decoder.codec_info().short_name.to_owned();
    let info = format.format_info();
    let container = info.short_name.to_owned();
    let dur_includes_trim = [FORMAT_ID_MP1, FORMAT_ID_MP2, FORMAT_ID_MP3].contains(&info.format);

    Ok(Opened {
        format,
        decoder,
        track_id,
        time_base,
        sample_rate,
        channels,
        bit_depth,
        layout,
        codec,
        codec_id,
        container,
        declared_frames,
        dur_includes_trim,
        gapless: None,
    })
}

/// Bit depth implied by IEEE-float PCM codec ids (symphonia leaves
/// `bits_per_sample` unset for them).
fn float_bits(codec: AudioCodecId) -> Option<u32> {
    use codec_ids::*;
    match codec {
        CODEC_ID_PCM_F32LE
        | CODEC_ID_PCM_F32BE
        | CODEC_ID_PCM_F32LE_PLANAR
        | CODEC_ID_PCM_F32BE_PLANAR => Some(32),
        CODEC_ID_PCM_F64LE
        | CODEC_ID_PCM_F64BE
        | CODEC_ID_PCM_F64LE_PLANAR
        | CODEC_ID_PCM_F64BE_PLANAR => Some(64),
        _ => None,
    }
}

/// ALAC bit depth from the magic cookie (symphonia's MP4 demuxer leaves
/// `bits_per_sample` unset). Parsed from the front like Apple's ALAC
/// decoder (`ALACDecoder::Init`, "magic cookie" description): an optional
/// 12-byte `frma` atom, then an optional 12-byte `alac` atom header, then
/// the 24-byte `ALACSpecificConfig` (anything after it, e.g. a `chan`
/// atom or a terminator, is ignored). `bitDepth` is its byte 5 and must be
/// 16, 20, 24 or 32.
fn alac_bits(codec: AudioCodecId, extra: Option<&[u8]>) -> Option<u32> {
    if codec != codec_ids::CODEC_ID_ALAC {
        return None;
    }
    let mut cookie = extra?;
    for tag in [b"frma", b"alac"] {
        if cookie.get(4..8) == Some(&tag[..]) {
            cookie = cookie.get(12..)?;
        }
    }
    let cookie = cookie.get(..24)?;
    let bits = u32::from(cookie[5]);
    [16, 20, 24, 32].contains(&bits).then_some(bits)
}

/// Converts a packet duration in `time_base` ticks to frames at `rate`.
/// Without a time base, ticks are frames (symphonia's audio convention).
pub(super) fn ticks_to_frames(ticks: u64, time_base: Option<TimeBase>, rate: u32) -> u64 {
    match time_base {
        Some(tb) if !(tb.numer.get() == 1 && tb.denom.get() == rate) => {
            let num = u128::from(ticks) * u128::from(tb.numer.get()) * u128::from(rate);
            let den = u128::from(tb.denom.get());
            u64::try_from((num + den / 2) / den).unwrap_or(u64::MAX)
        }
        _ => ticks,
    }
}

/// Maps a symphonia error for `path` to a core error.
pub(super) fn map_error(path: &Path, e: SymError) -> Error {
    match e {
        SymError::IoError(io) if io.kind() == io::ErrorKind::UnexpectedEof => {
            decode_err(path, "unexpected end of file")
        }
        SymError::IoError(io) => Error::io(path, io),
        SymError::Unsupported(what) => unsupported(path, what),
        other => decode_err(path, other.to_string()),
    }
}

pub(super) fn decode_err(path: &Path, reason: impl Into<String>) -> Error {
    Error::Decode {
        path: path.to_path_buf(),
        reason: reason.into(),
    }
}

fn unsupported(path: &Path, reason: impl Into<String>) -> Error {
    Error::UnsupportedFormat {
        path: path.to_path_buf(),
        reason: reason.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::num::NonZero;

    #[test]
    fn ticks_convert_through_time_base() {
        assert_eq!(ticks_to_frames(1_152, None, 44_100), 1_152);
        let recip = TimeBase::try_from_recip(44_100);
        assert_eq!(ticks_to_frames(1_152, recip, 44_100), 1_152);
        // 1 ms ticks at 48 kHz: 20 ticks = 960 frames.
        let ms = TimeBase::new(NonZero::new(1).unwrap(), NonZero::new(1_000).unwrap());
        assert_eq!(ticks_to_frames(20, Some(ms), 48_000), 960);
        assert_eq!(ticks_to_frames(u64::MAX, Some(ms), 768_000), u64::MAX);
    }

    #[test]
    fn errors_map_to_core_variants() {
        let p = Path::new("/x.wav");
        let eof = SymError::IoError(io::Error::from(io::ErrorKind::UnexpectedEof));
        assert!(
            matches!(map_error(p, eof), Error::Decode { reason, .. } if reason == "unexpected end of file")
        );
        let denied = SymError::IoError(io::Error::from(io::ErrorKind::PermissionDenied));
        assert!(matches!(map_error(p, denied), Error::Io { .. }));
        let unsup = SymError::Unsupported("wav: riff form is not wave");
        assert!(matches!(
            map_error(p, unsup),
            Error::UnsupportedFormat { .. }
        ));
        let bad = SymError::DecodeError("wav: invalid riff length");
        assert!(
            matches!(map_error(p, bad), Error::Decode { reason, .. } if reason.contains("invalid riff length"))
        );
        let limit = SymError::LimitError("too big");
        assert!(matches!(map_error(p, limit), Error::Decode { .. }));
    }

    #[test]
    fn float_codecs_report_bit_depth() {
        assert_eq!(float_bits(codec_ids::CODEC_ID_PCM_F32LE), Some(32));
        assert_eq!(float_bits(codec_ids::CODEC_ID_PCM_F64BE), Some(64));
        assert_eq!(float_bits(codec_ids::CODEC_ID_PCM_S16LE), None);
    }

    #[test]
    fn alac_bit_depth_comes_from_the_cookie() {
        let mut cookie = vec![0u8; 24];
        cookie[5] = 24;
        let alac = codec_ids::CODEC_ID_ALAC;
        assert_eq!(alac_bits(alac, Some(&cookie)), Some(24));
        let frma = [&12u32.to_be_bytes()[..], b"frma", b"alac"].concat();
        let alac_hdr = [&36u32.to_be_bytes()[..], b"alac", &[0; 4]].concat();
        let wrapped = [frma, alac_hdr, cookie.clone()].concat();
        assert_eq!(wrapped.len(), 48);
        assert_eq!(alac_bits(alac, Some(&wrapped)), Some(24));
        // Trailing data (a `chan` atom, a terminator) is ignored.
        let trailing = [cookie.clone(), vec![0, 0, 0, 8, 0, 0, 0, 0]].concat();
        assert_eq!(alac_bits(alac, Some(&trailing)), Some(24));
        cookie[5] = 7;
        assert_eq!(alac_bits(alac, Some(&cookie)), None);
        assert_eq!(alac_bits(alac, Some(&[16; 5])), None);
        assert_eq!(alac_bits(alac, None), None);
        assert_eq!(alac_bits(codec_ids::CODEC_ID_FLAC, Some(&[16; 24])), None);
    }

    #[test]
    fn missing_file_is_io_error() {
        let p = std::env::temp_dir().join("sonar-decode-missing-dir-x/none.wav");
        assert!(matches!(open(&p), Err(Error::Io { .. })));
    }
}
