//! Unit tests of [`super`]: the chunk walk, the overlay plan and the
//! patched byte source.

use super::*;
use std::io::Cursor;

fn chunk(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut c = id.to_vec();
    c.extend_from_slice(&(body.len() as u32).to_be_bytes());
    c.extend_from_slice(body);
    if body.len() % 2 == 1 {
        c.push(0);
    }
    c
}

fn form(chunks: &[Vec<u8>]) -> Vec<u8> {
    let body: Vec<u8> = chunks.concat();
    [
        &b"FORM"[..],
        &(4 + body.len() as u32).to_be_bytes(),
        b"AIFF",
        &body,
    ]
    .concat()
}

fn comm(frames: u32) -> Vec<u8> {
    let mut b = 1u16.to_be_bytes().to_vec();
    b.extend_from_slice(&frames.to_be_bytes());
    b.extend_from_slice(&[0; 12]);
    chunk(b"COMM", &b)
}

fn ssnd(offset: u32, data: &[u8]) -> Vec<u8> {
    let mut b = offset.to_be_bytes().to_vec();
    b.extend_from_slice(&[0; 4]);
    b.extend_from_slice(&vec![0xAA; offset as usize]);
    b.extend_from_slice(data);
    chunk(b"SSND", &b)
}

fn layout(bytes: &[u8]) -> Layout {
    read_layout(&mut Cursor::new(bytes)).unwrap().unwrap()
}

#[test]
fn walk_finds_comm_and_ssnd() {
    let f = form(&[comm(50), ssnd(0, &[1; 100])]);
    let l = layout(&f);
    assert_eq!(l.comm_frames, Some(50));
    assert_eq!(l.ssnd_frames(2), Some(50));
    assert_eq!(l.overlay(), None, "intact");
    assert!(l.complete);
    // With an offset, the frames start after it.
    let l = layout(&form(&[comm(10), ssnd(4, &[1; 20])]));
    assert_eq!(l.ssnd_frames(2), Some(10));
    // Not AIFF.
    let wav = [&b"RIFF"[..], &[0; 4], b"WAVE"].concat();
    assert_eq!(read_layout(&mut Cursor::new(wav)).unwrap(), None);
    assert_eq!(read_layout(&mut Cursor::new(b"FO")).unwrap(), None);
}

#[test]
fn overlay_keeps_a_cut_ssnd_and_drops_other_cut_chunks() {
    let f = form(&[comm(50), ssnd(0, &[1; 100]), chunk(b"ID3 ", &[2; 40])]);
    let ssnd_at = 12 + 26;
    // Cut inside SSND (30 of 100 sample bytes left): SSND keeps 8 + 30.
    let cut = &f[..ssnd_at + 16 + 30];
    let l = layout(cut);
    assert!(l.ssnd_cut && l.complete);
    assert_eq!(
        l.overlay(),
        Some(vec![(ssnd_at as u64 + 4, 38), (4, cut.len() as u32 - 8)])
    );
    // Original lengths still known.
    assert_eq!((l.comm_frames, l.ssnd_frames(2)), (Some(50), Some(50)));
    // Cut inside the trailing ID3 chunk: the FORM ends after SSND.
    let ssnd_end = ssnd_at + 16 + 100;
    let l = layout(&f[..ssnd_end + 20]);
    assert!(!l.ssnd_cut);
    assert_eq!(l.overlay(), Some(vec![(4, ssnd_end as u32 - 8)]));
    // Cut inside a chunk header after SSND.
    let l = layout(&f[..ssnd_end + 3]);
    assert_eq!(l.overlay(), Some(vec![(4, ssnd_end as u32 - 8)]));
    // Cut inside SSND's own header: no SSND survives.
    let l = layout(&f[..ssnd_at + 12]);
    assert_eq!(l.ssnd, None);
    assert_eq!(l.overlay(), Some(vec![(4, ssnd_at as u32 - 8)]));
    // Missing final pad byte of an odd SSND.
    let odd = form(&[comm(1), ssnd(0, &[1; 3])]);
    let l = layout(&odd[..odd.len() - 1]);
    assert_eq!(l.overlay(), Some(vec![(4, odd.len() as u32 - 9)]));
}

#[test]
fn malformed_or_endless_layouts_get_no_overlay() {
    // A chunk overrunning its FORM: symphonia's error stands.
    let mut f = form(&[comm(1), ssnd(0, &[1; 10])]);
    let at = 12 + 4;
    f[at..at + 4].copy_from_slice(&1_000u32.to_be_bytes());
    f.truncate(f.len() - 2);
    assert!(!layout(&f).complete);
    assert_eq!(layout(&f).overlay(), None);
    // More than MAX_CHUNKS chunks before the cut.
    let many: Vec<Vec<u8>> = (0..=MAX_CHUNKS).map(|_| chunk(b"JUNK", &[])).collect();
    let f = form(&many);
    let l = layout(&f[..f.len() - 4]);
    assert!(!l.complete);
    assert_eq!(l.overlay(), None);
}

#[test]
fn read_failure_during_the_walk_is_an_io_error() {
    use crate::decode::robust_tests::FailingSource;
    let path = Path::new("/Volumes/Gone/loop.aif");
    let f = form(&[comm(50), ssnd(0, &[1; 100])]);
    // Reads fail after 20 bytes: the FORM header (12) and the COMM chunk
    // header (8) succeed, the COMM body read fails mid-walk. An I/O error
    // (retried by the pipeline), not a permanent decode error.
    match prepare(path, FailingSource::new(f.clone(), 20)) {
        Err(Error::Io { .. }) => {}
        Err(e) => panic!("expected Error::Io, got {e}"),
        Ok(_) => panic!("expected Error::Io, got Ok"),
    }
    // Same bytes, no limit: the walk succeeds.
    let (_, l) = prepare(path, FailingSource::new(f, u64::MAX)).unwrap();
    assert_eq!(l.and_then(|l| l.comm_frames), Some(50));
}

#[test]
fn patched_source_rewrites_only_the_patched_bytes() {
    let bytes: Vec<u8> = (0..32u8).collect();
    let mut p = Patched::new(Cursor::new(bytes.clone()), &[(6, 0xA1B2_C3D4)]).unwrap();
    let mut out = Vec::new();
    // Small reads straddle the patch.
    let mut buf = [0u8; 5];
    loop {
        let n = p.read(&mut buf).unwrap();
        if n == 0 {
            break;
        }
        out.extend_from_slice(&buf[..n]);
    }
    let mut want = bytes.clone();
    want[6..10].copy_from_slice(&[0xA1, 0xB2, 0xC3, 0xD4]);
    assert_eq!(out, want);
    // Seeking into the middle of the patch.
    p.seek(SeekFrom::Start(8)).unwrap();
    let mut two = [0u8; 4];
    p.read_exact(&mut two).unwrap();
    assert_eq!(two, [0xC3, 0xD4, 10, 11]);
    assert_eq!(p.byte_len(), Some(32));
}
