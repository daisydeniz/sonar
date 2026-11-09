//! The packet loop: decode + downmix the analysis window, then count the
//! remaining frames without decoding them.
//!
//! Past the window only packet *durations* are summed (`Packet::dur`, the
//! number of valid frames after gapless trimming, in track time-base
//! ticks; converted to frames once at the end, so no per-packet rounding
//! drift). That costs one demuxer read per packet and no codec work, so the
//! true length of a 5-minute file is known for the price of reading its
//! bytes.
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

use std::io::ErrorKind;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use symphonia::core::errors::Error as SymError;

use super::mix::append_mean;
use super::source::{decode_err, map_error, ticks_to_frames, Opened};
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
}

/// Reusable scratch: planar f32 copy of one decoded packet.
pub(super) type Planar = Vec<Vec<f32>>;

/// Streams every packet of the selected track. The first `keep` frames are
/// downmixed into `mono` (source rate); later packets are only counted.
pub(super) fn run(
    path: &Path,
    src: &mut Opened,
    keep: usize,
    planar: &mut Planar,
    mono: &mut Vec<f32>,
    cancel: Option<&AtomicBool>,
) -> Result<Outcome> {
    let mut out = Outcome::default();
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
        if mono.len() >= keep {
            ticks_past_window = ticks_past_window.saturating_add(packet.dur.get());
            continue;
        }
        match src.decoder.decode(&packet) {
            Ok(buf) => {
                consecutive_errors = 0;
                let frames = buf.frames();
                buf.copy_to_vecs_planar::<f32>(planar);
                append_mean(planar, frames.min(keep - mono.len()), mono);
                decoded_frames = decoded_frames.saturating_add(frames as u64);
            }
            Err(SymError::DecodeError(msg)) => {
                consecutive_errors += 1;
                note(&mut out, format!("undecodable packet: {msg}"));
                out.partial = true;
                let dur = ticks_to_frames(packet.dur.get(), src.time_base, src.sample_rate);
                let gap = usize::try_from(dur).unwrap_or(usize::MAX);
                mono.resize(mono.len() + gap.min(keep - mono.len()), 0.0);
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

fn note(out: &mut Outcome, problem: String) {
    if out.problem.is_none() {
        out.problem = Some(problem);
    }
}
