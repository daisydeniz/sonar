//! ISO base media file (ISO/IEC 14496-12) box writing shared by the M4A
//! writers ([`m4a`](super::m4a) ALAC, [`aac`](super::aac) AAC-LC).
//!
//! Layout: `ftyp`, `mdat` (all samples as one chunk), `moov` (`mvhd`,
//! `trak` → `tkhd`, optional `edts`, `mdia` → `mdhd`, `hdlr` `soun`,
//! `minf` → `smhd`, `dinf`, `stbl` → `stsd`, `stts`, `stsc`, `stsz`,
//! `stco`; optional `udta`). The media timescale is the sample rate, so
//! `stts` durations are frames.

/// Everything about the single audio track except its samples.
#[derive(Debug, Clone)]
pub(super) struct Track {
    /// Sample rate = media and movie timescale (Hz).
    pub sample_rate: u32,
    /// Media duration: every frame the samples decode to, priming and
    /// padding included (`mdhd`).
    pub media_frames: u32,
    /// Presentation duration after edits (`mvhd`, `tkhd`).
    pub movie_frames: u32,
    /// Complete `stsd` box.
    pub stsd: Vec<u8>,
    /// Complete `stts` box.
    pub stts: Vec<u8>,
    /// Complete `edts` box, or empty.
    pub edts: Vec<u8>,
    /// Complete `udta` box, or empty.
    pub udta: Vec<u8>,
}

/// A whole file: `ftyp` (brand `M4A `), `mdat` holding `samples` back to
/// back, then `moov` describing `track`.
pub(super) fn file(samples: &[Vec<u8>], track: &Track) -> Vec<u8> {
    let ftyp = boxed(
        b"ftyp",
        &[&b"M4A "[..], &[0; 4], b"M4A ", b"mp42", b"isom"].concat(),
    );
    let mdat = boxed(b"mdat", &samples.concat());
    let chunk_offset = (ftyp.len() + 8) as u32;
    [ftyp, mdat, moov(track, samples, chunk_offset)].concat()
}

fn moov(t: &Track, samples: &[Vec<u8>], chunk_offset: u32) -> Vec<u8> {
    let rate = t.sample_rate;
    // Matrix: identity in 16.16 / 2.30 fixed point.
    let matrix: Vec<u8> = [0x0001_0000u32, 0, 0, 0, 0x0001_0000, 0, 0, 0, 0x4000_0000]
        .iter()
        .flat_map(|v| v.to_be_bytes())
        .collect();
    let mvhd = full_box(
        b"mvhd",
        0,
        &[
            &u32s(&[0, 0, rate, t.movie_frames, 0x0001_0000])[..],
            &0x0100u16.to_be_bytes(),
            &[0; 10],
            &matrix,
            &[0; 24],
            &2u32.to_be_bytes(), // next track id
        ]
        .concat(),
    );
    let tkhd = full_box(
        b"tkhd",
        7, // enabled | in movie | in preview
        &[
            &u32s(&[0, 0, 1, 0, t.movie_frames, 0, 0])[..],
            &[0, 0, 0, 0], // layer, alternate group
            &0x0100u16.to_be_bytes(),
            &[0, 0],
            &matrix,
            &[0; 8], // width, height
        ]
        .concat(),
    );
    let mdhd = full_box(
        b"mdhd",
        0,
        &[
            &u32s(&[0, 0, rate, t.media_frames])[..],
            &0x55C4u16.to_be_bytes(),
            &[0, 0],
        ]
        .concat(),
    );
    let hdlr = full_box(
        b"hdlr",
        0,
        &[&[0; 4][..], b"soun", &[0; 12], b"SoundHandler\0"].concat(),
    );
    let smhd = full_box(b"smhd", 0, &[0; 4]);
    let url = full_box(b"url ", 1, &[]); // self-contained
    let dref = full_box(b"dref", 0, &[&1u32.to_be_bytes()[..], &url].concat());
    let dinf = boxed(b"dinf", &dref);
    let stbl = boxed(
        b"stbl",
        &[
            t.stsd.clone(),
            t.stts.clone(),
            full_box(b"stsc", 0, &u32s(&[1, 1, samples.len() as u32, 1])),
            stsz(samples),
            full_box(b"stco", 0, &u32s(&[1, chunk_offset])),
        ]
        .concat(),
    );
    let minf = boxed(b"minf", &[smhd, dinf, stbl].concat());
    let mdia = boxed(b"mdia", &[mdhd, hdlr, minf].concat());
    let trak = boxed(b"trak", &[tkhd, t.edts.clone(), mdia].concat());
    boxed(b"moov", &[mvhd, trak, t.udta.clone()].concat())
}

/// `stsd` with one audio sample entry (version 0) of type `kind`; `config`
/// is the codec's configuration box (`alac`, `esds`). Rates ≥ 65536 Hz do
/// not fit the 16.16 field (the codec config carries the real rate).
pub(super) fn audio_stsd(
    kind: &[u8; 4],
    channels: u16,
    bits: u16,
    rate: u32,
    config: &[u8],
) -> Vec<u8> {
    let rate_fixed = if rate < 65_536 { rate << 16 } else { 0 };
    let entry = boxed(
        kind,
        &[
            &[0; 6][..],
            &1u16.to_be_bytes(), // data reference index
            &[0; 8],             // version, revision, vendor
            &channels.to_be_bytes(),
            &bits.to_be_bytes(),
            &[0; 4], // compression id, packet size
            &rate_fixed.to_be_bytes(),
            config,
        ]
        .concat(),
    );
    full_box(b"stsd", 0, &[&1u32.to_be_bytes()[..], &entry].concat())
}

/// `stts` for `packets` samples of `len` frames each except a last one of
/// `last` frames.
pub(super) fn stts(packets: usize, len: u32, last: u32) -> Vec<u8> {
    let mut runs = Vec::new();
    if last == len {
        runs.push((packets as u32, len));
    } else if packets > 0 {
        if packets > 1 {
            runs.push((packets as u32 - 1, len));
        }
        runs.push((1, last));
    }
    let mut body = u32s(&[runs.len() as u32]);
    for (count, delta) in runs {
        body.extend_from_slice(&u32s(&[count, delta]));
    }
    full_box(b"stts", 0, &body)
}

fn stsz(samples: &[Vec<u8>]) -> Vec<u8> {
    let mut body = u32s(&[0, samples.len() as u32]);
    for p in samples {
        body.extend_from_slice(&(p.len() as u32).to_be_bytes());
    }
    full_box(b"stsz", 0, &body)
}

/// Big-endian u32 sequence.
pub(super) fn u32s(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_be_bytes()).collect()
}

/// A box: 32-bit size, type, body.
pub(super) fn boxed(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let size = u32::try_from(8 + body.len()).unwrap_or(u32::MAX);
    [&size.to_be_bytes()[..], kind, body].concat()
}

/// A full box: version 0 + 24-bit `flags`, then the body.
pub(super) fn full_box(kind: &[u8; 4], flags: u32, body: &[u8]) -> Vec<u8> {
    boxed(
        kind,
        &[&(flags & 0xFF_FFFF).to_be_bytes()[..], body].concat(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stts_runs_cover_every_frame() {
        let total = |b: &[u8]| -> u64 {
            let n = u32::from_be_bytes([b[12], b[13], b[14], b[15]]) as usize;
            (0..n)
                .map(|i| {
                    let o = 16 + 8 * i;
                    let c = u32::from_be_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
                    let d = u32::from_be_bytes([b[o + 4], b[o + 5], b[o + 6], b[o + 7]]);
                    u64::from(c) * u64::from(d)
                })
                .sum()
        };
        assert_eq!(total(&stts(11, 4096, 44_100 - 10 * 4096)), 44_100);
        assert_eq!(total(&stts(2, 4096, 4096)), 8_192);
        assert_eq!(total(&stts(1, 4096, 100)), 100);
        assert_eq!(total(&stts(46, 1024, 1024)), 47_104);
    }

    #[test]
    fn file_starts_with_ftyp_then_mdat_at_the_chunk_offset() {
        let t = Track {
            sample_rate: 44_100,
            media_frames: 10,
            movie_frames: 10,
            stsd: audio_stsd(b"alac", 1, 16, 44_100, &[]),
            stts: stts(1, 10, 10),
            edts: Vec::new(),
            udta: Vec::new(),
        };
        let b = file(&[vec![1, 2, 3]], &t);
        assert_eq!(&b[4..8], b"ftyp");
        let ftyp_len = u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize;
        assert_eq!(&b[ftyp_len + 4..ftyp_len + 8], b"mdat");
        assert_eq!(&b[ftyp_len + 8..ftyp_len + 11], &[1, 2, 3]);
        assert!(b.windows(4).any(|w| w == b"moov"));
    }
}
