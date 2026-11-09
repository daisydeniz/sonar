//! Encoder priming/padding of MP4 audio, read from the file itself.
//!
//! symphonia 0.6.1's `isomp4` demuxer ignores edit lists (its source says
//! "TODO: Support edits") and `iTunSMPB`: an AAC-LC `.m4a` decodes to
//! `units × 1024` frames, i.e. 2,112 priming frames at the start and the
//! encoder's padding at the end are included. Every real AAC encoder
//! (Apple, ffmpeg, FDK) records them in an edit list (`moov/trak/edts/elst`,
//! ISO/IEC 14496-12 §8.6.6) and/or Apple's `iTunSMPB` freeform tag
//! (`moov/udta/meta/ilst/----`), so this module parses just those boxes
//! with seeks (the sample tables are never read) and reports how many
//! frames to drop at the start and how many are playable.
//! `stream::tests::symphonia_isomp4_ignores_priming_metadata` pins the
//! upstream behaviour: if symphonia starts trimming, that test fails and
//! this module must go (otherwise priming would be removed twice).
//!
//! Precedence: a single edit with `media_time ≥ 0` wins (ffmpeg / ISO
//! semantics: `segment_duration` is honoured, unlike Apple's reader, which
//! takes only the start from it); if `iTunSMPB` agrees on the priming its
//! sample-exact length replaces the edit's (movie-timescale) one. Without a
//! usable edit, `iTunSMPB` (priming, original length) applies. Anything
//! else — several edits, an empty edit (initial silence), absurd values —
//! is ignored, never an error: the file then decodes untrimmed, as before.
//! Only the track symphonia selected (matched by `tkhd` id) is read;
//! `iTunSMPB` is file-wide. `udta/meta` is read both as an ISO full box and
//! as a QuickTime plain box.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// Largest priming honoured (frames). Real values are 1,105–4,096; the
/// bound keeps a hostile edit list from making the decoder buffer
/// gigabytes before the first kept sample.
pub(super) const MAX_START: u64 = 1 << 17;
/// Leaf boxes larger than this are skipped (`iTunSMPB` data is < 200 B).
const MAX_LEAF: u64 = 1 << 20;
/// Boxes inspected per level before giving up (a `moov` has a handful).
const MAX_BOXES: usize = 4096;

/// Frames to drop from the start of the decoded stream and playable frames
/// after that, at the track's sample rate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Gapless {
    /// Priming frames to discard.
    pub start: u64,
    /// Playable frames after `start`; `None` = until the stream ends.
    pub len: Option<u64>,
}

/// Reads the priming metadata of track `track_id` of the MP4 file at
/// `path`; `None` if there is none, it is unsupported, or the file cannot
/// be parsed.
pub(super) fn read_isomp4(path: &Path, sample_rate: u32, track_id: u32) -> Option<Gapless> {
    let mut f = File::open(path).ok()?;
    let end = f.seek(SeekFrom::End(0)).ok()?;
    let raw = parse(&mut f, end, track_id)?;
    raw.resolve(sample_rate)
}

/// What was found in the file.
#[derive(Debug, Default, PartialEq)]
struct Raw {
    /// Movie timescale (`mvhd`).
    movie_ts: u32,
    /// Media timescale of the audio track (`mdhd`).
    media_ts: u32,
    /// Edit list entries `(segment_duration, media_time)` of the audio track.
    edits: Vec<(u64, i64)>,
    /// `iTunSMPB`: `(priming, padding, valid length)`.
    smpb: Option<(u64, u64, u64)>,
}

impl Raw {
    fn resolve(&self, rate: u32) -> Option<Gapless> {
        let conv = |x: u64, ts: u32| -> Option<u64> {
            let ts = u128::from(ts);
            if ts == 0 {
                return None;
            }
            u64::try_from((u128::from(x) * u128::from(rate) + ts / 2) / ts).ok()
        };
        // A failed conversion (missing timescale…) only loses the edit
        // candidate, never `iTunSMPB`.
        let edit = || -> Option<Gapless> {
            let [(dur, time)] = self.edits.as_slice() else {
                return None;
            };
            Some(Gapless {
                start: conv(u64::try_from(*time).ok()?, self.media_ts)?,
                len: if *dur == 0 {
                    None
                } else {
                    Some(conv(*dur, self.movie_ts)?)
                },
            })
        };
        let useful = |g: &Gapless| g.start <= MAX_START && (g.start > 0 || g.len.is_some());
        let edit = edit().filter(useful);
        let smpb = self
            .smpb
            .map(|(start, _padding, len)| Gapless {
                start,
                len: (len > 0).then_some(len),
            })
            .filter(useful);
        match (edit, smpb) {
            // `segment_duration` is in the (often coarse) movie timescale;
            // iTunSMPB's length is sample exact.
            (Some(e), Some(s)) if e.start == s.start => Some(Gapless {
                start: s.start,
                len: s.len.or(e.len),
            }),
            (Some(e), _) => Some(e),
            (None, s) => s,
        }
    }
}

/// A box header: type and body range.
#[derive(Debug, Clone, Copy)]
struct Hdr {
    kind: [u8; 4],
    body: u64,
    end: u64,
}

/// Header of the box at `pos` inside `..limit`; `None` at the end or when
/// the size is invalid.
fn header<R: Read + Seek>(r: &mut R, pos: u64, limit: u64) -> Option<Hdr> {
    if pos.checked_add(8)? > limit {
        return None;
    }
    r.seek(SeekFrom::Start(pos)).ok()?;
    let mut h = [0u8; 8];
    r.read_exact(&mut h).ok()?;
    let size = u64::from(u32::from_be_bytes([h[0], h[1], h[2], h[3]]));
    let (len, hdr) = match size {
        0 => (limit - pos, 8),
        1 => {
            let mut b = [0u8; 8];
            r.read_exact(&mut b).ok()?;
            (u64::from_be_bytes(b), 16)
        }
        n => (n, 8),
    };
    let end = pos.checked_add(len)?;
    (len >= hdr && end <= limit).then_some(Hdr {
        kind: [h[4], h[5], h[6], h[7]],
        body: pos + hdr,
        end,
    })
}

/// The child boxes in `body..end`.
fn children<R: Read + Seek>(r: &mut R, body: u64, end: u64) -> Vec<Hdr> {
    let mut out = Vec::new();
    let mut pos = body;
    while out.len() < MAX_BOXES {
        let Some(h) = header(r, pos, end) else { break };
        pos = h.end;
        out.push(h);
    }
    out
}

fn find(hs: &[Hdr], kind: &[u8; 4]) -> Option<Hdr> {
    hs.iter().find(|h| &h.kind == kind).copied()
}

/// The body of a small leaf box.
fn leaf<R: Read + Seek>(r: &mut R, h: Hdr) -> Option<Vec<u8>> {
    let len = h.end - h.body;
    if len > MAX_LEAF {
        return None;
    }
    r.seek(SeekFrom::Start(h.body)).ok()?;
    let mut v = vec![0u8; usize::try_from(len).ok()?];
    r.read_exact(&mut v).ok()?;
    Some(v)
}

fn be32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(
        b.get(at..at.checked_add(4)?)?.try_into().ok()?,
    ))
}

fn be64(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_be_bytes(
        b.get(at..at.checked_add(8)?)?.try_into().ok()?,
    ))
}

/// `mvhd`/`mdhd` timescale (version 0: after 2 × 4-byte times; 1: 2 × 8).
fn timescale(b: &[u8]) -> Option<u32> {
    be32(b, if *b.first()? == 1 { 20 } else { 12 })
}

/// `elst` entries `(segment_duration, media_time)`.
fn elst(b: &[u8]) -> Option<Vec<(u64, i64)>> {
    let v1 = *b.first()? == 1;
    let (size, n) = (if v1 { 20 } else { 12 }, be32(b, 4)? as usize);
    let mut out = Vec::new();
    for i in 0..n.min(b.len() / size) {
        let at = 8 + i * size;
        out.push(if v1 {
            (be64(b, at)?, be64(b, at + 8)? as i64)
        } else {
            (u64::from(be32(b, at)?), i64::from(be32(b, at + 4)? as i32))
        });
    }
    Some(out)
}

/// `iTunSMPB` text → `(priming, padding, valid length)`: whitespace
/// separated hex fields `0, priming, padding, length, …`.
fn smpb(text: &str) -> Option<(u64, u64, u64)> {
    let mut f = text
        .split_whitespace()
        .map(|t| u64::from_str_radix(t, 16).ok());
    f.next()??;
    Some((f.next()??, f.next()??, f.next()??))
}

fn parse<R: Read + Seek>(r: &mut R, end: u64, track_id: u32) -> Option<Raw> {
    let top = children(r, 0, end);
    let moov = find(&top, b"moov")?;
    let kids = children(r, moov.body, moov.end);
    let mut raw = Raw::default();
    if let Some(b) = find(&kids, b"mvhd").and_then(|h| leaf(r, h)) {
        raw.movie_ts = timescale(&b)?;
    }
    // The audio (`soun`) track with symphonia's track id.
    for trak in kids.iter().filter(|h| &h.kind == b"trak") {
        let tk = children(r, trak.body, trak.end);
        // `tkhd` track id: after version/flags and two times (v0: 4 B, v1: 8 B).
        let id = find(&tk, b"tkhd")
            .and_then(|h| leaf(r, h))
            .and_then(|b| be32(&b, if *b.first()? == 1 { 20 } else { 12 }));
        if id.is_some_and(|id| id != track_id) {
            continue;
        }
        let Some(mdia) = find(&tk, b"mdia") else {
            continue;
        };
        let md = children(r, mdia.body, mdia.end);
        let is_audio = find(&md, b"hdlr")
            .and_then(|h| leaf(r, h))
            .is_some_and(|b| b.get(8..12) == Some(&b"soun"[..]));
        if !is_audio {
            continue;
        }
        if let Some(b) = find(&md, b"mdhd").and_then(|h| leaf(r, h)) {
            raw.media_ts = timescale(&b)?;
        }
        if let Some(edts) = find(&tk, b"edts") {
            let es = children(r, edts.body, edts.end);
            if let Some(b) = find(&es, b"elst").and_then(|h| leaf(r, h)) {
                raw.edits = elst(&b)?;
            }
        }
        break;
    }
    raw.smpb = find_smpb(r, &kids);
    Some(raw)
}

/// `iTunSMPB` in `moov/udta/meta/ilst` (or `moov/meta/ilst`).
fn find_smpb<R: Read + Seek>(r: &mut R, moov_kids: &[Hdr]) -> Option<(u64, u64, u64)> {
    let mut metas = Vec::new();
    if let Some(m) = find(moov_kids, b"meta") {
        metas.push(m);
    }
    if let Some(u) = find(moov_kids, b"udta") {
        metas.extend(
            children(r, u.body, u.end)
                .into_iter()
                .filter(|h| &h.kind == b"meta"),
        );
    }
    for meta in metas {
        // ISO `meta` is a full box (skip version/flags); QuickTime's is a
        // plain box.
        let ilst = find(&children(r, meta.body + 4, meta.end), b"ilst")
            .or_else(|| find(&children(r, meta.body, meta.end), b"ilst"));
        let Some(ilst) = ilst else {
            continue;
        };
        for item in children(r, ilst.body, ilst.end) {
            if &item.kind != b"----" {
                continue;
            }
            let parts = children(r, item.body, item.end);
            let name = find(&parts, b"name").and_then(|h| leaf(r, h));
            if name.as_deref().and_then(|b| b.get(4..)) != Some(&b"iTunSMPB"[..]) {
                continue;
            }
            // `data`: version/flags (4), locale (4), text.
            let data = find(&parts, b"data").and_then(|h| leaf(r, h))?;
            return smpb(std::str::from_utf8(data.get(8..)?).ok()?);
        }
    }
    None
}

#[cfg(test)]
#[path = "gapless_tests.rs"]
mod tests;
