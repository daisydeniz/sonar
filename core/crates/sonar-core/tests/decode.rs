//! Decodes the whole generated fixture set (`sonar fixtures`, incl. the
//! 310 s file) and checks every file against the manifest: format, exact
//! duration in frames, analysis length, levels, and the corrupt-file
//! `expect` contract from `docs/EVAL.md`.
//!
//! Run in release with `--nocapture` to see decode throughput.

use std::time::Instant;

use sonar_core::decode::{DecodeOptions, Decoder, ANALYSIS_RATE, DEFAULT_MAX_ANALYSIS_SECS};
use sonar_core::fixtures::manifest::{Expect, Group};
use sonar_core::fixtures::{generate, FixtureOptions};
use sonar_core::Error;

#[test]
fn every_fixture_decodes_per_manifest() {
    let tmp = tempfile::tempdir().unwrap();
    let manifest = generate(tmp.path(), &FixtureOptions::default()).unwrap();
    let opts = DecodeOptions::default();
    let mut decoder = Decoder::new();
    let (mut ok, mut corrupt, mut audio_secs) = (0, 0, 0.0);
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
        assert_eq!(s.container, "wave", "{id}");
        assert_eq!(s.sample_rate, fmt.sample_rate, "{id}");
        assert_eq!(s.channels, fmt.channels, "{id}");
        assert_eq!(s.bit_depth, Some(u32::from(fmt.bit_depth)), "{id}");
        assert_eq!(s.num_frames, fmt.num_frames, "{id}: true length");
        assert_eq!(s.declared_frames, Some(fmt.num_frames), "{id}");
        assert!((s.duration_s - fmt.duration_s).abs() < 1e-6, "{id}");
        audio_secs += s.duration_s;

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
    assert!(corrupt >= 15 && ok >= 90, "{ok} audio, {corrupt} corrupt");
}
