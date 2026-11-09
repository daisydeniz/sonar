//! Tests of [`super`]: hand-built MP4 files (only the boxes it reads).

use super::*;
use std::io::Cursor;

fn boxed(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    [&(8 + body.len() as u32).to_be_bytes()[..], kind, body].concat()
}

fn full(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    boxed(kind, &[&[0u8; 4][..], body].concat())
}

fn u32s(v: &[u32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_be_bytes()).collect()
}

/// One audio `trak`: id, media timescale, `elst` entries
/// `(segment_duration, media_time)`.
fn trak(id: u32, media_ts: u32, edits: &[(u32, i32)]) -> Vec<u8> {
    let tkhd = full(b"tkhd", &u32s(&[0, 0, id, 0, 1_000]));
    let mdhd = full(b"mdhd", &u32s(&[0, 0, media_ts, 1_000]));
    let hdlr = full(b"hdlr", &[&[0u8; 4][..], b"soun", &[0; 13]].concat());
    let mdia = boxed(b"mdia", &[mdhd, hdlr].concat());
    let mut entries = u32s(&[edits.len() as u32]);
    for (d, t) in edits {
        entries.extend(u32s(&[*d, *t as u32, 0x0001_0000]));
    }
    let edts = if edits.is_empty() {
        vec![]
    } else {
        boxed(b"edts", &full(b"elst", &entries))
    };
    boxed(b"trak", &[tkhd, edts, mdia].concat())
}

/// `udta/meta/ilst/----` with `iTunSMPB`; `plain_meta` = QuickTime style.
fn udta(text: &str, plain_meta: bool) -> Vec<u8> {
    let item = boxed(
        b"----",
        &[
            full(b"mean", b"com.apple.iTunes"),
            full(b"name", b"iTunSMPB"),
            full(b"data", &[&[0u8; 4][..], text.as_bytes()].concat()),
        ]
        .concat(),
    );
    let ilst = boxed(b"ilst", &item);
    let meta = if plain_meta {
        boxed(b"meta", &ilst)
    } else {
        full(b"meta", &ilst)
    };
    boxed(b"udta", &meta)
}

fn mp4(movie_ts: u32, traks: &[Vec<u8>], udta: Option<Vec<u8>>) -> Vec<u8> {
    let mvhd = full(b"mvhd", &u32s(&[0, 0, movie_ts, 1_000]));
    let moov = [vec![mvhd], traks.to_vec(), udta.into_iter().collect()].concat();
    [
        boxed(b"ftyp", b"M4A \0\0\0\0"),
        boxed(b"mdat", &[0; 64]),
        boxed(b"moov", &moov.concat()),
    ]
    .concat()
}

/// One track (id 1, 44.1 kHz media and movie timescale).
fn file(edits: &[(u32, i32)], smpb_text: Option<&str>) -> Vec<u8> {
    mp4(
        44_100,
        &[trak(1, 44_100, edits)],
        smpb_text.map(|t| udta(t, false)),
    )
}

fn read(bytes: Vec<u8>, rate: u32) -> Option<Gapless> {
    read_track(bytes, rate, 1)
}

fn read_track(bytes: Vec<u8>, rate: u32, track: u32) -> Option<Gapless> {
    let end = bytes.len() as u64;
    parse(&mut Cursor::new(bytes), end, track)?.resolve(rate)
}

const SMPB: &str = " 00000000 00000840 000001A0 0000000000004E20 00000000";

fn gapless(start: u64, len: u64) -> Option<Gapless> {
    Some(Gapless {
        start,
        len: Some(len),
    })
}

#[test]
fn single_edit_gives_priming_and_length() {
    assert_eq!(
        read(file(&[(20_000, 2112)], None), 44_100),
        gapless(2112, 20_000)
    );
}

#[test]
fn itunsmpb_alone_gives_priming_and_valid_length() {
    assert_eq!(read(file(&[], Some(SMPB)), 44_100), gapless(2112, 20_000));
}

#[test]
fn edit_list_wins_when_it_disagrees_with_itunsmpb() {
    let f = file(&[(1_000, 100)], Some(SMPB));
    assert_eq!(read(f, 44_100), gapless(100, 1_000));
}

#[test]
fn itunsmpb_length_is_preferred_when_the_priming_agrees() {
    // Movie timescale 1,000: 455 ms = 20,066 frames at 44.1 kHz, but
    // iTunSMPB says 20,000 exactly.
    let t = trak(1, 44_100, &[(455, 2112)]);
    let f = mp4(1_000, std::slice::from_ref(&t), None);
    assert_eq!(read(f, 44_100), gapless(2112, 20_066));
    let f = mp4(1_000, &[t], Some(udta(SMPB, false)));
    assert_eq!(read(f, 44_100), gapless(2112, 20_000));
}

#[test]
fn a_broken_edit_does_not_lose_itunsmpb() {
    // Missing media timescale.
    let f = mp4(
        44_100,
        &[trak(1, 0, &[(20_000, 2112)])],
        Some(udta(SMPB, false)),
    );
    assert_eq!(read(f, 44_100), gapless(2112, 20_000));
    // Missing movie timescale.
    let f = mp4(
        0,
        &[trak(1, 44_100, &[(20_000, 2112)])],
        Some(udta(SMPB, false)),
    );
    assert_eq!(read(f, 44_100), gapless(2112, 20_000));
    // Priming beyond MAX_START.
    let f = file(&[(100, 1 << 30)], Some(SMPB));
    assert_eq!(read(f, 44_100), gapless(2112, 20_000));
}

#[test]
fn quicktime_plain_meta_box_is_read() {
    let f = mp4(44_100, &[trak(1, 44_100, &[])], Some(udta(SMPB, true)));
    assert_eq!(read(f, 44_100), gapless(2112, 20_000));
}

#[test]
fn only_the_selected_track_is_read() {
    let f = mp4(
        44_100,
        &[
            trak(1, 44_100, &[(1_000, 100)]),
            trak(2, 44_100, &[(2_000, 200)]),
        ],
        None,
    );
    assert_eq!(read_track(f.clone(), 44_100, 1), gapless(100, 1_000));
    assert_eq!(read_track(f.clone(), 44_100, 2), gapless(200, 2_000));
    assert_eq!(read_track(f, 44_100, 3), None);
}

#[test]
fn unsupported_edits_fall_back_or_are_ignored() {
    // Empty edit (initial silence) and multiple edits: not honoured.
    assert_eq!(read(file(&[(500, -1), (1_000, 0)], None), 44_100), None);
    assert_eq!(read(file(&[(500, 0), (1_000, 600)], None), 44_100), None);
    assert_eq!(read(file(&[(500, -1)], None), 44_100), None);
    // …but iTunSMPB still applies.
    let g = read(file(&[(500, 0), (1_000, 600)], Some(SMPB)), 44_100).unwrap();
    assert_eq!(g.start, 2112);
    // An edit that trims nothing is no gapless data at all.
    assert_eq!(read(file(&[(0, 0)], None), 44_100), None);
    assert_eq!(read(file(&[], None), 44_100), None);
}

#[test]
fn absurd_priming_is_ignored() {
    assert_eq!(read(file(&[(100, 1 << 30)], None), 44_100), None);
}

#[test]
fn timescales_are_converted_to_the_sample_rate() {
    // 1,000 media ticks at a 22,050 Hz timescale = 2,000 frames at 44.1 kHz;
    // segment_duration is in the movie timescale (44.1 kHz): unchanged.
    let f = mp4(44_100, &[trak(1, 22_050, &[(20_000, 1_000)])], None);
    assert_eq!(read(f, 44_100), gapless(2_000, 20_000));
}

#[test]
fn truncated_and_garbage_input_never_panics() {
    let full = file(&[(20_000, 2112)], Some(SMPB));
    for n in 0..full.len() {
        let _ = read(full[..n].to_vec(), 44_100);
    }
    let mut junk = full.clone();
    for i in (0..junk.len()).step_by(3) {
        junk[i] = junk[i].wrapping_mul(31).wrapping_add(7);
        let _ = read(junk.clone(), 44_100);
    }
}

#[test]
fn smpb_text_parsing() {
    assert_eq!(smpb(SMPB), Some((2112, 416, 20_000)));
    assert_eq!(smpb(" 0 zz 1 2"), None);
    assert_eq!(smpb(" 0 1"), None);
}
