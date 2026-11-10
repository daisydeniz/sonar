//! AIFF length rules that symphonia 0.6.1 does not apply (T1.7d), read
//! from the file's chunk headers with a bounded, seek-only walk (the sound
//! data is never read).
//!
//! References: Apple *Audio Interchange File Format "AIFF"* 1.3 (1989) and
//! the *AIFF-C* draft (1991). A file is one `FORM` chunk (`ckSize` BE,
//! form type `AIFF`/`AIFC`) holding chunks `id`, `ckSize` (BE), body,
//! padded to an even length. `COMM` starts with `numChannels` (i16) and
//! `numSampleFrames` (u32); `SSND` starts with `offset` and `blockSize`
//! (u32), then the sample frames.
//!
//! Rules:
//! - **`COMM` `numSampleFrames` is the declared length** (the spec defines
//!   it as the number of sample frames in `SSND`); symphonia derives the
//!   length from the `SSND` size instead. An `SSND` holding more frames
//!   than `numSampleFrames` is capped after counting: the packets past
//!   `numSampleFrames` are still read and counted like any other, then the
//!   total (and the decoded output) is capped to `numSampleFrames` via a
//!   gapless length (the extra bytes are not sample frames per the spec;
//!   capped, not `partial`). An
//!   `SSND` holding fewer is a stream shorter than its header declares
//!   (`partial`, like a truncated WAV). `numSampleFrames = 0` means a
//!   writer that never finalised the header: with an intact `FORM` the
//!   `SSND` size is used; with a cut `FORM` the sizes are placeholders too
//!   (e.g. `FORM` 0xFFFFFFF8, `SSND` 0xFFFFFF00), so the length is unknown
//!   (no declared length, not `partial`, like a streamed WAV's 0xFFFFFFFF
//!   `data` size) and the surviving frames are counted.
//! - **Truncated `FORM`** (the `FORM` claims more bytes than the file
//!   holds): symphonia 0.6.1 skips the `SSND` body while scanning for
//!   chunks and fails with "unexpected end of file" when it ends early. The
//!   file is therefore read through [`Patched`], which presents a
//!   self-consistent header: an `SSND` cut by the end of the file keeps
//!   its surviving bytes (its `ckDataSize` and the `FORM` end at the end of
//!   the file); any other chunk cut by the end of the file (or a partial
//!   chunk header) is dropped by ending the `FORM` before it. The declared
//!   length stays the original one (`COMM`, else the original `SSND`
//!   size), so the result is the exact surviving frame count and
//!   `partial`. Nothing else in the file changes.
//!
//! The walk runs on the same open file the demuxer then reads
//! ([`prepare`]): one open per file; a non-AIFF file costs one 12-byte read
//! and a rewind.

use std::io::{self, ErrorKind, Read, Seek, SeekFrom};
use std::path::Path;

use symphonia::core::io::MediaSource;

use super::gapless::Gapless;
use super::source::Opened;
use crate::{Error, Result};

/// Chunks inspected before giving up (real files have a handful).
const MAX_CHUNKS: usize = 4096;

/// What the chunk walk found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Layout {
    /// File length (bytes).
    file_len: u64,
    /// End of the `FORM` chunk as declared (8 + `ckSize`).
    form_end: u64,
    /// `COMM` `numSampleFrames`, if a `COMM` body was readable.
    comm_frames: Option<u64>,
    /// The `SSND` chunk, if its 16-byte header was readable.
    ssnd: Option<Ssnd>,
    /// End (without pad byte) of the last chunk wholly inside the file.
    whole_end: u64,
    /// The `SSND` chunk is the one cut by the end of the file.
    ssnd_cut: bool,
    /// The walk reached the end of the `FORM` or the cut (not the
    /// [`MAX_CHUNKS`] limit or a chunk overrunning the `FORM`).
    complete: bool,
}

/// An `SSND` chunk header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Ssnd {
    /// Offset of the chunk id in the file.
    at: u64,
    /// `ckDataSize` as stored.
    size: u32,
    /// `offset` field (bytes skipped before the first frame).
    offset: u32,
}

impl Layout {
    /// The `FORM` claims more bytes than the file holds.
    fn truncated(&self) -> bool {
        self.form_end > self.file_len
    }

    /// Frames the original `SSND` declares, `frame_bytes` per frame (what
    /// symphonia derives from an intact file: whole frames only).
    fn ssnd_frames(&self, frame_bytes: u64) -> Option<u64> {
        let s = self.ssnd?;
        let payload = u64::from(s.size).checked_sub(8 + u64::from(s.offset))?;
        payload.checked_div(frame_bytes)
    }

    /// Header patches `(file offset, new BE u32)` that make a truncated
    /// file self-consistent (module doc); `None` if the `FORM` is intact or
    /// the layout is not understood.
    fn overlay(&self) -> Option<Vec<(u64, u32)>> {
        if !self.truncated() || !self.complete {
            return None;
        }
        let mut patches = Vec::with_capacity(2);
        let mut end = self.whole_end;
        if let Some(s) = self.ssnd.filter(|_| self.ssnd_cut) {
            // Body bytes present; symphonia needs at least offset + 8.
            let body = self.file_len.checked_sub(s.at + 8)?;
            if body >= 8 + u64::from(s.offset) {
                patches.push((s.at + 4, u32::try_from(body).ok()?));
                end = self.file_len;
            }
        }
        patches.push((4, u32::try_from(end.checked_sub(8)?).ok()?));
        Some(patches)
    }
}

/// Walks the chunk headers of `r` (positioned at its start) and rewinds
/// it; `Ok(None)` if it is not an AIFF/AIFF-C file. Only real read failures
/// are errors (end of file is a finding).
fn read_layout<R: Read + Seek>(r: &mut R) -> io::Result<Option<Layout>> {
    let mut head = [0u8; 12];
    let is_aiff = match r.read_exact(&mut head) {
        Ok(()) => &head[..4] == b"FORM" && (&head[8..] == b"AIFF" || &head[8..] == b"AIFC"),
        Err(e) if e.kind() == ErrorKind::UnexpectedEof => false,
        Err(e) => return Err(e),
    };
    let layout = if is_aiff { Some(walk(r, &head)?) } else { None };
    r.seek(SeekFrom::Start(0))?;
    Ok(layout)
}

/// The chunk walk proper, after the 12-byte `FORM` header `head`.
fn walk<R: Read + Seek>(r: &mut R, head: &[u8; 12]) -> io::Result<Layout> {
    let file_len = r.seek(SeekFrom::End(0))?;
    let form_end = 8 + u64::from(be32(&head[4..8]));
    let mut l = Layout {
        file_len,
        form_end,
        comm_frames: None,
        ssnd: None,
        whole_end: 12,
        ssnd_cut: false,
        complete: false,
    };
    let truncated = form_end > file_len;
    let mut pos = 12u64;
    for _ in 0..MAX_CHUNKS {
        // Same end rule as symphonia's chunk reader: < 8 bytes left in the
        // FORM ends the walk.
        if pos + 8 > form_end {
            l.complete = true;
            break;
        }
        let mut h = [0u8; 8];
        if !read_at(r, pos, &mut h)? {
            l.complete = true; // header cut by the end of the file
            break;
        }
        let size = be32(&h[4..]);
        let end = pos + 8 + u64::from(size);
        if end > form_end {
            break; // overruns its parent: malformed, leave it to symphonia
        }
        let mut body = [0u8; 8];
        let readable = read_at(r, pos + 8, &mut body[..size.min(8) as usize])?;
        match &h[..4] {
            b"COMM" if size >= 6 && readable => {
                l.comm_frames = Some(u64::from(be32(&body[2..6])));
            }
            b"SSND" if size >= 8 && readable => {
                l.ssnd = Some(Ssnd {
                    at: pos,
                    size,
                    offset: be32(&body[..4]),
                });
            }
            _ => {}
        }
        if end > file_len {
            l.ssnd_cut = &h[..4] == b"SSND";
            l.complete = true;
            break;
        }
        l.whole_end = end;
        if !truncated && l.comm_frames.is_some() && l.ssnd.is_some() {
            l.complete = true;
            break;
        }
        pos = end + u64::from(size & 1);
    }
    Ok(l)
}

/// Prepares the freshly opened `source` (positioned at its start) for the
/// demuxer: walks it if it is an AIFF and wraps it in [`Patched`] when its
/// `FORM` is cut by the end of the file (module doc). Returns the walk for
/// [`apply`]. Read failures are [`Error::Io`].
pub(super) fn prepare<S: MediaSource + 'static>(
    path: &Path,
    mut source: S,
) -> Result<(Box<dyn MediaSource>, Option<Layout>)> {
    let layout = read_layout(&mut source).map_err(|e| Error::io(path, e))?;
    match layout.and_then(|l| l.overlay()) {
        Some(patches) => {
            let patched = Patched::new(source, &patches).map_err(|e| Error::io(path, e))?;
            Ok((Box::new(patched), layout))
        }
        None => Ok((Box::new(source), layout)),
    }
}

/// Applies the `COMM` length rules (module doc) to an AIFF opened from the
/// source [`prepare`] walked.
pub(super) fn apply(src: &mut Opened, layout: &Layout) {
    // What symphonia will read: whole frames of the (patched) SSND.
    // (Every AIFF codec symphonia 0.6.1 supports is PCM, so the frame size
    // is known.)
    let readable = src.declared_frames;
    let ssnd = src
        .pcm_frame_bytes
        .and_then(|fb| layout.ssnd_frames(fb))
        .or(readable);
    let declared = match layout.comm_frames {
        // Never finalised: with a cut FORM the SSND size is a placeholder.
        Some(0) if layout.truncated() => None,
        Some(0) if ssnd.is_some_and(|n| n > 0) => ssnd,
        Some(n) => Some(n),
        None => ssnd,
    };
    src.declared_frames = declared;
    if let (Some(d), Some(n)) = (declared, readable) {
        if n > d {
            src.gapless = Some(Gapless {
                start: 0,
                len: Some(d),
            });
        }
    }
}

fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

/// Fills `buf` from offset `at`; `Ok(false)` if the file ends first.
fn read_at<R: Read + Seek>(r: &mut R, at: u64, buf: &mut [u8]) -> io::Result<bool> {
    r.seek(SeekFrom::Start(at))?;
    match r.read_exact(buf) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == ErrorKind::UnexpectedEof => Ok(false),
        Err(e) => Err(e),
    }
}

/// A byte source with a few 4-byte header fields replaced (BE u32), for
/// symphonia only: the file itself is never written.
pub(super) struct Patched<S> {
    inner: S,
    pos: u64,
    patches: Vec<(u64, [u8; 4])>,
}

impl<S: Seek> Patched<S> {
    /// Wraps `inner` (rewound to the start) with `patches`.
    pub(super) fn new(mut inner: S, patches: &[(u64, u32)]) -> io::Result<Self> {
        inner.seek(SeekFrom::Start(0))?;
        Ok(Self {
            inner,
            pos: 0,
            patches: patches
                .iter()
                .map(|&(at, v)| (at, v.to_be_bytes()))
                .collect(),
        })
    }
}

impl<S: Read> Read for Patched<S> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        let (lo, hi) = (self.pos, self.pos + n as u64);
        for (at, bytes) in &self.patches {
            for (p, b) in (*at..).zip(bytes) {
                if (lo..hi).contains(&p) {
                    if let Some(slot) = usize::try_from(p - lo).ok().and_then(|i| buf.get_mut(i)) {
                        *slot = *b;
                    }
                }
            }
        }
        self.pos = hi;
        Ok(n)
    }
}

impl<S: Seek> Seek for Patched<S> {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        self.pos = self.inner.seek(to)?;
        Ok(self.pos)
    }
}

impl<S: MediaSource> MediaSource for Patched<S> {
    fn is_seekable(&self) -> bool {
        self.inner.is_seekable()
    }
    fn byte_len(&self) -> Option<u64> {
        self.inner.byte_len()
    }
}

#[cfg(test)]
#[path = "aiff_tests.rs"]
mod tests;
