//! The packet loop: decode + downmix the analysis window, then count the
//! remaining frames without decoding them.
//!
//! With a [`Meter`] (basic meta, [`crate::meta`]), packets are decoded
//! until the meter's own window is full too, and every decoded block's
//! playable frames (priming dropped, gapless end respected) are fed to it
//! at the source rate, all channels, before downmix. Only the analysis
//! window is ever buffered; the mono samples are identical with or without
//! a meter. A codec error on a packet decoded only for the meter stops the
//! meter ([`Outcome::meter_problem`]) and the loop continues counting, so
//! `partial` and `problem` are identical with or without a meter, and so is
//! the length except, for compressed codecs, on a truncated file whose
//! short final packet lies past the analysis window (decoded frames <
//! declared duration; ADR-012). PCM in WAVE/AIFF is counted by its bytes
//! ([`packet_ticks`]), so there decoded and counted lengths always agree.
//!
//! Past the window only packet *durations* are summed (`Packet::dur`, the
//! number of valid frames after gapless trimming, in track time-base
//! ticks; converted to frames once at the end, so no per-packet rounding
//! drift; PCM WAVE/AIFF: the frames its bytes hold). That costs one demuxer
//! read per packet and no codec work, so the true length of a 5-minute
//! file is known for the price of reading its bytes.
//!
//! Error policy (consistent with `docs/EVAL.md` corrupt `expect`):
//! - a real read failure (anything but end-of-file: drive unplugged,
//!   network volume gone, permission revoked) is returned as
//!   [`Error::Io`] — never a short "partial" result, so the pipeline can
//!   retry the file instead of storing a wrong duration;
//! - a packet the codec rejects (`DecodeError`) is replaced by silence of
//!   the packet's duration (keeps later audio time-aligned) and marks the
//!   result `partial`; more than [`MAX_CONSECUTIVE_ERRORS`] in a row stops
//!   decoding;
//! - a malformed-stream error from the demuxer stops reading; the result
//!   is `partial` (a plain end-of-file only if the stream is shorter than
//!   its header declares);
//! - if nothing at all was decoded and an error occurred, the caller gets
//!   that error instead of an empty result.
//!
//! MP4 priming ([`super::gapless`]): the leading `start` frames of the
//! decoded stream are dropped and the total is capped at the edit's length,
//! so the counted length is `min(raw − start, len)` on both paths. An AIFF
//! whose `SSND` holds more frames than `COMM` declares is capped the same
//! way (`start` = 0, [`super::aiff`]).

use std::io::ErrorKind;
use std::ops::Range;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use symphonia::core::errors::Error as SymError;
use symphonia::core::packet::Packet;

use super::mix::append_mean;
use super::source::{decode_err, map_error, ticks_to_frames, Opened};
use crate::meta::Meter;
use crate::{Error, Result};

/// Consecutive undecodable packets tolerated before giving up.
pub const MAX_CONSECUTIVE_ERRORS: u32 = 32;
/// Packets read before giving up (guards against a demuxer that never
/// reaches the end). The smallest packet of any enabled codec is a 16-frame
/// FLAC block, so a file of [`super::HARD_MAX_SOURCE_FRAMES`] (2^26) frames
/// needs at most 2^22 packets; 2^23 leaves 2× headroom and still covers
/// ~60 h of 44.1 kHz PCM (1,152-frame packets) for duration counting.
pub const MAX_PACKETS: u64 = 1 << 23;

/// What the loop found.
#[derive(Debug, Default)]
pub(super) struct Outcome {
    /// Playable frames in the file (decoded + counted past the window).
    pub total_frames: u64,
    /// Some audio is missing or was replaced by silence.
    pub partial: bool,
    /// Why the result is partial (first problem seen).
    pub problem: Option<String>,
    /// Metering stopped early: a packet past the analysis window could not
    /// be decoded (the rest of the file was only counted).
    pub meter_problem: Option<String>,
}

/// Reusable scratch: planar f32 copy of one decoded packet.
pub(super) type Planar = Vec<Vec<f32>>;

/// Streams every packet of the selected track. The first `keep` frames are
/// downmixed into `mono` (source rate); `meter`, if given, measures the
/// playable frames until it is full; later packets are only counted.
pub(super) fn run(
    path: &Path,
    src: &mut Opened,
    keep: usize,
    planar: &mut Planar,
    mono: &mut Vec<f32>,
    mut meter: Option<&mut Meter>,
    cancel: Option<&AtomicBool>,
) -> Result<Outcome> {
    let mut out = Outcome::default();
    // Priming (MP4 edit list / iTunSMPB): `skip` leading frames are decoded
    // like any other and dropped at the end, so they do not count towards
    // the window. A zero window (length probe) decodes nothing at all.
    let skip = src
        .gapless
        .map_or(0, |g| usize::try_from(g.start).unwrap_or(0));
    // Playable span in decoded-stream frames, for the meter.
    let play_start = src.gapless.map_or(0, |g| g.start);
    let play_end = src
        .gapless
        .and_then(|g| g.len)
        .map_or(u64::MAX, |len| play_start.saturating_add(len));
    let keep = if keep == 0 {
        0
    } else {
        keep.saturating_add(skip)
    };
    let mut decoded_frames = 0u64;
    let mut ticks_past_window = 0u64;
    let mut consecutive_errors = 0u32;
    let mut packets = 0u64;
    let mut ended_by_error = false;
    loop {
        if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
            return Err(Error::Cancelled);
        }
        packets += 1;
        if packets > MAX_PACKETS {
            note(&mut out, format!("stopped after {MAX_PACKETS} packets"));
            ended_by_error = true;
            break;
        }
        let packet = match src.format.next_packet() {
            Ok(Some(p)) => p,
            Ok(None) => break,
            Err(SymError::IoError(io)) if io.kind() != ErrorKind::UnexpectedEof => {
                return Err(Error::io(path, io));
            }
            Err(e) => {
                // Plain EOF is how a truncated stream ends; whether that
                // lost audio is decided below from the declared length.
                if !matches!(&e, SymError::IoError(_)) {
                    ended_by_error = true;
                }
                note(&mut out, map_error(path, e).to_string());
                break;
            }
        };
        if packet.track_id != src.track_id {
            continue;
        }
        let meter_wants = meter.as_ref().is_some_and(|m| !m.is_full()) && decoded_frames < play_end;
        let metering_only = mono.len() >= keep;
        if metering_only && !meter_wants {
            let ticks = packet_ticks(&packet, src);
            ticks_past_window = ticks_past_window.saturating_add(ticks);
            continue;
        }
        match src.decoder.decode(&packet) {
            // Past the analysis window a packet is decoded only for the
            // meter. Any codec failure there stops metering and falls back
            // to counting (this packet included), exactly what `decode()`
            // does, so the decode result never depends on the meter.
            Err(e) if metering_only => {
                out.meter_problem
                    .get_or_insert_with(|| map_error(path, e).to_string());
                meter = None;
                let ticks = packet_ticks(&packet, src);
                ticks_past_window = ticks_past_window.saturating_add(ticks);
            }
            Ok(buf) => {
                consecutive_errors = 0;
                let frames = buf.frames();
                buf.copy_to_vecs_planar::<f32>(planar);
                append_mean(planar, frames.min(keep - mono.len()), mono);
                if let Some(m) = meter.as_deref_mut() {
                    let r = playable(decoded_frames, frames as u64, play_start, play_end);
                    m.push(planar, r);
                }
                decoded_frames = decoded_frames.saturating_add(frames as u64);
            }
            Err(SymError::DecodeError(msg)) => {
                consecutive_errors += 1;
                note(&mut out, format!("undecodable packet: {msg}"));
                out.partial = true;
                let ticks = packet_ticks(&packet, src);
                let dur = ticks_to_frames(ticks, src.time_base, src.sample_rate);
                let gap = usize::try_from(dur).unwrap_or(usize::MAX);
                mono.resize(mono.len() + gap.min(keep - mono.len()), 0.0);
                if let Some(m) = meter.as_deref_mut() {
                    m.push_silence(playable(decoded_frames, dur, play_start, play_end).len());
                }
                decoded_frames = decoded_frames.saturating_add(dur);
                if consecutive_errors > MAX_CONSECUTIVE_ERRORS {
                    ended_by_error = true;
                    break;
                }
            }
            Err(SymError::IoError(io)) if io.kind() != ErrorKind::UnexpectedEof => {
                return Err(Error::io(path, io));
            }
            Err(e) => {
                note(&mut out, map_error(path, e).to_string());
                ended_by_error = true;
                break;
            }
        }
    }

    let past = ticks_to_frames(ticks_past_window, src.time_base, src.sample_rate);
    out.total_frames = decoded_frames.saturating_add(past);
    if let Some(g) = src.gapless {
        // Drop the priming from the front, the padding from the end.
        out.total_frames = out.total_frames.saturating_sub(g.start);
        if let Some(len) = g.len {
            out.total_frames = out.total_frames.min(len);
        }
        mono.drain(..skip.min(mono.len()));
        mono.truncate(usize::try_from(out.total_frames).unwrap_or(usize::MAX));
    }
    let short = src.declared_frames.is_some_and(|n| out.total_frames < n);
    if short && out.problem.is_none() {
        note(&mut out, "stream shorter than its header declares".into());
    }
    out.partial |= ended_by_error || short;
    if out.total_frames == 0 && (ended_by_error || short) {
        let reason = out.problem.take().unwrap_or_default();
        return Err(decode_err(path, format!("no audio decoded: {reason}")));
    }
    if !out.partial {
        out.problem = None;
    }
    Ok(out)
}

/// Part of a block of `n` frames starting at decoded-stream frame `pos`
/// that lies in the playable span `[lo, hi)`, as indices into the block.
fn playable(pos: u64, n: u64, lo: u64, hi: u64) -> Range<usize> {
    let start = lo.saturating_sub(pos).min(n);
    let end = hi.saturating_sub(pos).min(n).max(start);
    let idx = |v: u64| usize::try_from(v).unwrap_or(usize::MAX);
    idx(start)..idx(end)
}

fn note(out: &mut Outcome, problem: String) {
    if out.problem.is_none() {
        out.problem = Some(problem);
    }
}

/// Frames (= ticks) `packet` holds for uncompressed PCM in WAVE/AIFF, its
/// [`valid_ticks`] otherwise.
///
/// symphonia 0.6.1's RIFF demuxer (`symphonia-format-riff`
/// `common::next_packet`) sets `dur` to the whole blocks it asks for, but
/// reads them with `read_boxed_slice`, which returns a short buffer at end
/// of file without an error: the final packet of a file cut inside it
/// would count up to 1,151 phantom frames. The PCM decoder decodes
/// `data.len() / frame bytes` whole frames (`symphonia-codec-pcm`
/// `decode_inner`), so counting the bytes gives exactly what decoding
/// would, and a trailing partial frame is dropped on both paths (T1.7d).
fn packet_ticks(packet: &Packet, src: &Opened) -> u64 {
    match src.pcm_frame_bytes {
        Some(fb) if fb > 0 => packet.data.len() as u64 / fb,
        _ => valid_ticks(packet, src.dur_includes_trim),
    }
}

/// Playable (gapless-trimmed) duration of `packet` in time-base ticks.
///
/// symphonia's packet contract is `dur` = frames left after removing
/// `trim_start`/`trim_end`, and its decoders trim their output by those
/// amounts. symphonia 0.6.1's `PacketBuilder::trimmed_dur` — used only by
/// the MPEG audio demuxer — computes the trims but subtracts the builder's
/// (still zero) fields from the block length, so MP3 packets report the
/// untrimmed length in `dur`. Counting those past the analysis window
/// would include the LAME encoder delay and padding. For such demuxers
/// (`dur_includes_trim`) the trims are subtracted here.
/// `tests::symphonia_mp3_packet_dur_is_untrimmed` pins the upstream
/// behaviour so a fixed symphonia fails loudly instead of double-trimming.
pub(super) fn valid_ticks(packet: &Packet, dur_includes_trim: bool) -> u64 {
    let dur = packet.dur.get();
    if dur_includes_trim {
        dur.saturating_sub(packet.trim_start.get())
            .saturating_sub(packet.trim_end.get())
    } else {
        dur
    }
}

#[cfg(test)]
mod tests {
    use super::super::source::open_source;
    use super::*;
    use symphonia::core::packet::PacketBuilder;
    use symphonia::core::units::{Duration, Timestamp};

    /// Upstream pin (symphonia 0.6.1): the MP4 demuxer ignores the edit list
    /// and `iTunSMPB`, so an AAC file reports and decodes every sample of
    /// the media, priming and padding included. If this fails, symphonia
    /// trims itself: drop `decode::gapless` and its use in
    /// `source::open` (priming would otherwise be removed twice).
    #[test]
    fn symphonia_isomp4_ignores_priming_metadata() {
        use crate::fixtures::aac::{encode, AacSpec, Tags};
        use std::io::Cursor;
        let spec = AacSpec {
            sample_rate: 44_100,
            channels: 1,
            frames: 20_000,
            tags: Tags::Both,
            burst_unit: None,
        };
        let bytes = encode(&spec).unwrap();
        let mut src = open_source(Path::new("x.m4a"), Box::new(Cursor::new(bytes))).unwrap();
        assert_eq!(
            src.declared_frames,
            Some(22 * 1024),
            "untrimmed media length"
        );
        let p = src.format.next_packet().unwrap().unwrap();
        assert_eq!(
            (p.dur.get(), p.trim_start.get(), p.trim_end.get()),
            (1024, 0, 0)
        );
        assert!(src.gapless.is_none(), "open_source does not read priming");
    }

    /// Upstream pin (symphonia 0.6.1): `trimmed_dur` sets correct trims but
    /// an untrimmed `dur`. If this fails, symphonia fixed it: drop the
    /// `dur_includes_trim` workaround in [`valid_ticks`] and `source.rs`.
    #[test]
    fn symphonia_mp3_packet_dur_is_untrimmed() {
        let p = PacketBuilder::new()
            .track_id(0)
            .pts(Timestamp::from(-10i64))
            .trimmed_dur(Duration::from(100u64), Some(Timestamp::from(50i64)))
            .data(vec![0u8; 4])
            .build();
        assert_eq!(p.trim_start.get(), 10);
        assert_eq!(p.trim_end.get(), 40);
        assert_eq!(p.dur.get(), 100, "symphonia now trims dur itself");
        assert_eq!(valid_ticks(&p, true), 50);
        assert_eq!(valid_ticks(&p, false), 100);
    }
}
