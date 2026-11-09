//! Decodes the whole generated fixture set (`sonar fixtures`, incl. the
//! 310 s file and every container in `containers/`) and checks every file
//! against the manifest: container, format, exact duration in frames (also
//! when only counting packets), analysis length, levels, bit-exact samples
//! for lossless files, and the corrupt-file `expect` contract from
//! `docs/EVAL.md`.
//!
//! Run in release with `--nocapture` to see decode throughput.

use std::collections::BTreeSet;
use std::time::Instant;

use sonar_core::decode::{
    decode_file, DecodeOptions, Decoder, ANALYSIS_RATE, DEFAULT_MAX_ANALYSIS_SECS,
};
use sonar_core::fixtures::manifest::{Container, Expect, Fixture, Group};
use sonar_core::fixtures::{generate, stored_pcm, FixtureOptions, Manifest};
use sonar_core::Error;

/// symphonia's container short name for each fixture container.
fn symphonia_container(c: Container) -> &'static str {
    match c {
        Container::Wav => "wave",
        Container::Aiff => "aiff",
        Container::Flac => "flac",
        Container::Mp3 => "mp3",
        Container::Ogg => "ogg",
        Container::M4a => "isomp4",
    }
}

fn small_set() -> (tempfile::TempDir, Manifest) {
    let tmp = tempfile::tempdir().unwrap();
    let opts = FixtureOptions {
        include_long: false,
        ..FixtureOptions::default()
    };
    let manifest = generate(tmp.path(), &opts).unwrap();
    (tmp, manifest)
}

fn audio(m: &Manifest) -> impl Iterator<Item = &Fixture> {
    m.fixtures.iter().filter(|f| f.format.is_some())
}

#[test]
fn every_fixture_decodes_per_manifest() {
    let tmp = tempfile::tempdir().unwrap();
    let manifest = generate(tmp.path(), &FixtureOptions::default()).unwrap();
    let opts = DecodeOptions::default();
    let mut decoder = Decoder::new();
    let (mut ok, mut corrupt, mut audio_secs) = (0, 0, 0.0);
    let mut extensions = BTreeSet::new();
    let started = Instant::now();

    for f in &manifest.fixtures {
        let path = tmp.path().join(&f.path);
        let result = decoder.decode(&path, &opts);
        let id = &f.path;

        if let Some(c) = &f.truth.corrupt {
            corrupt += 1;
            let outcome = match &result {
                Ok(d) => format!("ok (partial={}, {} frames)", d.partial, d.source.num_frames),
                Err(e) => format!("error: {e}"),
            };
            eprintln!("{id:<32} {:<16} → {outcome}", format!("{:?}", c.expect));
            match (&c.expect, &result) {
                (_, Err(Error::Decode { .. } | Error::UnsupportedFormat { .. })) => {}
                (Expect::ErrorOrPartial, Ok(_)) => {}
                (want, got) => panic!("{id}: expected {want:?}, got {got:?}"),
            }
            continue;
        }

        let d = result.unwrap_or_else(|e| panic!("{id}: {e}"));
        let fmt = f.format.as_ref().unwrap();
        let s = &d.source;
        assert!(!d.partial, "{id}: partial ({:?})", d.problem);
        assert_eq!(s.container, symphonia_container(fmt.container), "{id}");
        assert_eq!(s.sample_rate, fmt.sample_rate, "{id}");
        assert_eq!(s.channels, fmt.channels, "{id}");
        assert_eq!(s.bit_depth, fmt.bit_depth.map(u32::from), "{id}");
        assert_eq!(s.num_frames, fmt.num_frames, "{id}: true length");
        assert_eq!(s.declared_frames, Some(fmt.num_frames), "{id}");
        assert!((s.duration_s - fmt.duration_s).abs() < 1e-6, "{id}");
        audio_secs += s.duration_s;
        if let Some((_, ext)) = f.path.rsplit_once('.') {
            extensions.insert(ext.to_owned());
        }

        let rate = u64::from(fmt.sample_rate);
        let window = (DEFAULT_MAX_ANALYSIS_SECS * rate as f64).ceil() as u64;
        assert_eq!(d.truncated_for_analysis, fmt.num_frames > window, "{id}");
        let full_len = (fmt.num_frames * u64::from(ANALYSIS_RATE)).div_ceil(rate);
        let cap = (DEFAULT_MAX_ANALYSIS_SECS * f64::from(ANALYSIS_RATE)) as u64;
        let want_len = full_len.min(cap);
        let got_len = d.samples.len() as u64;
        assert!(
            got_len.abs_diff(want_len) <= 1,
            "{id}: {got_len} vs {want_len} samples"
        );

        let peak = d.samples.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        assert!(d.samples.iter().all(|x| x.is_finite()), "{id}");
        if f.truth.is_silent == Some(true) {
            // Also every lossy fixture: they encode digital silence.
            assert_eq!(peak, 0.0, "{id}: silence must decode to zeros");
        } else if fmt.num_frames > 0 {
            assert!(peak > 0.0, "{id}: non-silent file decoded silent");
        }
        // Sines (identical channels) keep their written peak through
        // downmix + band-limited resampling.
        if f.group == Group::Formats {
            let want_db = f.truth.peak_dbfs.unwrap();
            let got_db = 20.0 * f64::from(peak).log10();
            assert!(
                (got_db - want_db).abs() < 0.1,
                "{id}: peak {got_db:.3} vs {want_db:.3} dBFS"
            );
        }
        ok += 1;
    }

    let elapsed = started.elapsed().as_secs_f64();
    let files = ok + corrupt;
    eprintln!(
        "decoded {files} files ({ok} audio, {corrupt} corrupt, {audio_secs:.0} s of audio) in {elapsed:.3} s: \
         {:.0} files/s, {:.0}x realtime",
        files as f64 / elapsed,
        audio_secs / elapsed
    );
    assert!(manifest
        .fixtures
        .iter()
        .any(|f| f.format.as_ref().is_some_and(|x| x.duration_s > 300.0)));
    assert!(corrupt >= 15 && ok >= 100, "{ok} audio, {corrupt} corrupt");
    for ext in sonar_core::scan::AUDIO_EXTENSIONS {
        assert!(extensions.contains(*ext), "no decoded .{ext} fixture");
    }
}

/// A zero-length window decodes nothing and counts every packet's
/// (gapless-trimmed) duration: the length must still be exact. This is the
/// path every frame past the 30 s analysis window takes.
#[test]
fn packet_counting_alone_gives_the_exact_length() {
    let (tmp, manifest) = small_set();
    let opts = DecodeOptions {
        max_analysis_secs: Some(0.0),
        ..DecodeOptions::default()
    };
    for f in audio(&manifest) {
        let fmt = f.format.as_ref().unwrap();
        let d = decode_file(&tmp.path().join(&f.path), &opts)
            .unwrap_or_else(|e| panic!("{}: {e}", f.path));
        assert!(d.samples.is_empty(), "{}", f.path);
        assert!(!d.partial, "{}: partial ({:?})", f.path, d.problem);
        assert_eq!(d.source.num_frames, fmt.num_frames, "{}", f.path);
    }
}

/// Lossless files (AIFF, FLAC, ALAC and integer WAV) decoded at their own
/// rate (no resampling) reproduce the stored integers exactly.
///
/// The decoder returns the mono mean of the channels. Every expected value
/// is exact in `f32`: a sample is `q / 2^(b−1)` with `|q| < 2^23`, and the
/// stereo mean `(q0 + q1) / 2^b` has `|q0 + q1| ≤ 2^24`, which `f32`
/// (24-bit significand) represents exactly; halving is exact too.
#[test]
fn lossless_fixtures_decode_bit_exactly_at_native_rate() {
    let (tmp, manifest) = small_set();
    let mut checked = BTreeSet::new();
    for f in audio(&manifest) {
        let Some(pcm) = stored_pcm(&f.path, manifest.seed) else {
            continue;
        };
        let opts = DecodeOptions {
            target_rate: pcm.sample_rate,
            max_analysis_secs: None,
            ..DecodeOptions::default()
        };
        let d = decode_file(&tmp.path().join(&f.path), &opts)
            .unwrap_or_else(|e| panic!("{}: {e}", f.path));
        assert_eq!(d.samples.len(), pcm.frames(), "{}", f.path);
        let ch = usize::from(pcm.channels);
        assert!(ch <= 2, "{}: expectation assumes ≤ 2 channels", f.path);
        let denom = pcm.scale() * ch as f64;
        for (i, (frame, &got)) in pcm.samples.chunks_exact(ch).zip(&d.samples).enumerate() {
            let sum: i64 = frame.iter().map(|&q| i64::from(q)).sum();
            let want = (sum as f64 / denom) as f32;
            assert!(
                got == want,
                "{}: frame {i}: got {got:e}, want {want:e} (q = {frame:?})",
                f.path
            );
        }
        if let Some((_, ext)) = f.path.rsplit_once('.') {
            checked.insert(ext.to_owned());
        }
    }
    let want: BTreeSet<String> = ["wav", "aif", "aiff", "flac", "m4a"]
        .map(str::to_owned)
        .into();
    assert_eq!(checked, want);
}

/// Lossy fixtures are digital silence and must decode to exact zeros at
/// their own rate too (no resampler involved).
#[test]
fn lossy_fixtures_decode_to_silence_at_native_rate() {
    let (tmp, manifest) = small_set();
    let mut n = 0;
    for f in audio(&manifest) {
        let fmt = f.format.as_ref().unwrap();
        if fmt.lossless {
            continue;
        }
        let opts = DecodeOptions {
            target_rate: fmt.sample_rate,
            max_analysis_secs: None,
            ..DecodeOptions::default()
        };
        let d = decode_file(&tmp.path().join(&f.path), &opts)
            .unwrap_or_else(|e| panic!("{}: {e}", f.path));
        assert_eq!(d.samples.len() as u64, fmt.num_frames, "{}", f.path);
        assert!(d.samples.iter().all(|&x| x == 0.0), "{}", f.path);
        n += 1;
    }
    assert!(n >= 4, "{n} lossy fixtures");
}
