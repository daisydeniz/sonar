//! Tests for [`super::lead`]: the analysis window past a long leading
//! silence, and its error handling (ADR-013).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use super::lead::{first_sound, PRE_ROLL_S};
use super::tests::{wav, Tmp};
use super::*;
use crate::fixtures::wav::SampleFormat;
use crate::Error;

const RATE: u32 = ANALYSIS_RATE;

/// `silence_s` of digital silence, then `sound_s` of 0.5-amplitude clicks
/// (10 ms every 0.5 s), mono at the analysis rate.
fn late_clicks(silence_s: f64, sound_s: f64) -> Vec<u8> {
    let rate = f64::from(RATE);
    let lead = (silence_s * rate) as usize;
    let total = lead + (sound_s * rate) as usize;
    let (period, click) = ((0.5 * rate) as usize, (0.01 * rate) as usize);
    let samples = (0..total)
        .map(|i| {
            if i >= lead && (i - lead) % period < click {
                0.5
            } else {
                0.0
            }
        })
        .collect();
    wav(RATE, SampleFormat::Int16, &[samples])
}

#[test]
fn first_sound_finds_the_first_loud_sample() {
    assert_eq!(first_sound(&[0.0, 1e-4, -0.01, 0.5]), Some(2));
    assert_eq!(first_sound(&[0.0, 1e-4, f32::NAN]), None);
    assert_eq!(first_sound(&[]), None);
}

#[test]
fn long_lead_moves_the_window_to_the_first_sound() {
    let tmp = Tmp::new();
    let p = tmp.write("late.wav", &late_clicks(40.0, 20.0));
    let opts = DecodeOptions::default();
    let mut dec = Decoder::new();
    let first = dec.decode(&p, &opts).unwrap();
    assert!(first.truncated_for_analysis);
    assert_eq!(first_sound(&first.samples), None);
    let d = dec.skip_leading_silence(&p, &opts, first).unwrap();
    assert!(
        (d.window_start_s - 39.95).abs() < 1e-3,
        "{}",
        d.window_start_s
    );
    assert!(d.truncated_for_analysis);
    // The window opens on the pre-roll, then the clicks (~20.05 s left).
    let pre = (PRE_ROLL_S * f64::from(RATE)) as usize;
    assert!(first_sound(&d.samples).is_some_and(|i| i.abs_diff(pre) <= 1));
    let len_s = d.samples.len() as f64 / f64::from(RATE);
    assert!((len_s - 20.05).abs() < 0.01, "{len_s}");
    assert!((d.source.duration_s - 60.0).abs() < 1e-3);
}

#[test]
fn short_file_keeps_its_leading_silence() {
    let tmp = Tmp::new();
    let p = tmp.write("short.wav", &late_clicks(10.0, 15.0));
    let opts = DecodeOptions::default();
    let mut dec = Decoder::new();
    let first = dec.decode(&p, &opts).unwrap();
    assert!(!first.truncated_for_analysis);
    let n = first.samples.len();
    let d = dec.skip_leading_silence(&p, &opts, first).unwrap();
    assert_eq!(d.window_start_s, 0.0);
    assert_eq!(d.samples.len(), n);
    assert!(!d.truncated_for_analysis);
}

#[test]
fn silent_long_file_keeps_its_first_window() {
    let tmp = Tmp::new();
    let p = tmp.write("silent.wav", &late_clicks(45.0, 0.0));
    let opts = DecodeOptions::default();
    let mut dec = Decoder::new();
    let first = dec.decode(&p, &opts).unwrap();
    let n = first.samples.len();
    let d = dec.skip_leading_silence(&p, &opts, first).unwrap();
    assert_eq!(d.window_start_s, 0.0);
    assert_eq!(d.samples.len(), n);
    assert!(d.truncated_for_analysis);
}

#[test]
fn cancel_during_the_redecode_is_returned() {
    let tmp = Tmp::new();
    let p = tmp.write("late.wav", &late_clicks(40.0, 20.0));
    let cancel = Arc::new(AtomicBool::new(false));
    let opts = DecodeOptions {
        cancel: Some(Arc::clone(&cancel)),
        ..DecodeOptions::default()
    };
    let mut dec = Decoder::new();
    let first = dec.decode(&p, &opts).unwrap();
    cancel.store(true, Ordering::Relaxed);
    let r = dec.skip_leading_silence(&p, &opts, first);
    assert!(matches!(r, Err(Error::Cancelled)), "{r:?}");
}

#[test]
fn vanished_file_during_the_redecode_is_an_io_error() {
    let tmp = Tmp::new();
    let p = tmp.write("late.wav", &late_clicks(40.0, 20.0));
    let opts = DecodeOptions::default();
    let mut dec = Decoder::new();
    let first = dec.decode(&p, &opts).unwrap();
    std::fs::remove_file(&p).unwrap();
    let r = dec.skip_leading_silence(&p, &opts, first);
    assert!(matches!(r, Err(Error::Io { .. })), "{r:?}");
}

#[test]
fn corrupt_redecode_falls_back_to_the_first_window() {
    let tmp = Tmp::new();
    let p = tmp.write("late.wav", &late_clicks(40.0, 20.0));
    let opts = DecodeOptions::default();
    let mut dec = Decoder::new();
    let first = dec.decode(&p, &opts).unwrap();
    let n = first.samples.len();
    std::fs::write(&p, b"not audio at all").unwrap();
    let d = dec.skip_leading_silence(&p, &opts, first).unwrap();
    assert_eq!(d.window_start_s, 0.0);
    assert_eq!(d.samples.len(), n);
}
