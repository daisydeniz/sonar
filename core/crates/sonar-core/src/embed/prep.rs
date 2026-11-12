//! File -> CLAP features: decode the first 10 s of the file from its
//! **native rate** to 48 kHz mono (ADR-026: the 22.05 kHz analysis decode
//! upsampled would lose 11-14 kHz and shift the embedding), then the front
//! end. One per worker thread.

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use super::front::{FrontEnd, CLIP_SAMPLES, SAMPLE_RATE};
use crate::decode::{DecodeOptions, Decoder};
use crate::Result;

/// Seconds decoded (the clip length of the model).
const CLIP_SECS: f64 = CLIP_SAMPLES as f64 / SAMPLE_RATE as f64;

/// Features of one file, with the decoder's verdict on its integrity.
#[derive(Debug, Clone, PartialEq)]
pub struct Prepared {
    /// [`super::FEATURE_LEN`] log-mel values, time-major.
    pub features: Vec<f32>,
    /// The file is damaged or truncated; the features cover what was readable.
    pub partial: bool,
    /// First problem seen when `partial`.
    pub problem: Option<String>,
}

/// Decoder + front end with reusable scratch state.
#[derive(Debug, Default)]
pub struct AudioPrep {
    decoder: Decoder,
    front: FrontEnd,
}

impl AudioPrep {
    /// New instance with empty caches.
    pub fn new() -> Self {
        Self::default()
    }

    /// Decodes `path` and computes its features.
    ///
    /// # Errors
    /// Decode errors ([`crate::Error::Decode`], [`crate::Error::Io`],
    /// [`crate::Error::Cancelled`], ...); [`crate::Error::Embed`] for a
    /// file without audio frames.
    pub fn prepare(&mut self, path: &Path, cancel: Option<Arc<AtomicBool>>) -> Result<Prepared> {
        let opts = DecodeOptions {
            target_rate: SAMPLE_RATE,
            max_analysis_secs: Some(CLIP_SECS),
            cancel,
        };
        let decoded = self.decoder.decode(path, &opts)?;
        let features = self.front.features(&decoded.samples)?;
        Ok(Prepared {
            features,
            partial: decoded.partial,
            problem: decoded.problem,
        })
    }

    /// Drops scratch state after a caught panic.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::wav::{encode, SampleFormat, WavSpec};
    use crate::{embed::FEATURE_LEN, Error};

    fn write_wav(dir: &Path, name: &str, rate: u32, secs: f32) -> std::path::PathBuf {
        let n = (rate as f32 * secs) as usize;
        let samples: Vec<f32> = (0..n)
            .map(|i| 0.5 * (i as f32 * 1500.0 * std::f32::consts::TAU / rate as f32).sin())
            .collect();
        let path = dir.join(name);
        std::fs::write(
            &path,
            encode(&WavSpec::new(rate, 1, SampleFormat::Int16), &samples).bytes,
        )
        .expect("write");
        path
    }

    #[test]
    fn native_rates_all_land_on_the_same_features_shape_and_band() {
        let tmp = tempfile::tempdir().expect("tmp");
        let mut prep = AudioPrep::new();
        let mut peaks = Vec::new();
        for rate in [22_050, 44_100, 48_000] {
            let p = write_wav(tmp.path(), &format!("t{rate}.wav"), rate, 1.0);
            let out = prep.prepare(&p, None).expect("prepare");
            assert_eq!(out.features.len(), FEATURE_LEN);
            assert!(!out.partial);
            // Hottest band of the first (tone) frames: same band for every
            // source rate, i.e. the resampling to 48 kHz is correct.
            let row = &out.features[10 * 64..11 * 64];
            peaks.push((0..64).fold(0, |b, i| if row[i] > row[b] { i } else { b }));
        }
        assert!(peaks.iter().all(|&p| p == peaks[0]), "{peaks:?}");
    }

    #[test]
    fn long_files_are_cropped_and_garbage_is_an_error() {
        let tmp = tempfile::tempdir().expect("tmp");
        let mut prep = AudioPrep::new();
        let long = write_wav(tmp.path(), "long.wav", 8_000, 15.0);
        assert_eq!(
            prep.prepare(&long, None).expect("long").features.len(),
            FEATURE_LEN
        );

        let bad = tmp.path().join("bad.wav");
        std::fs::write(&bad, b"not audio at all").expect("write");
        assert!(prep.prepare(&bad, None).is_err());
        let missing = tmp.path().join("missing.wav");
        assert!(matches!(
            prep.prepare(&missing, None),
            Err(Error::Io { .. })
        ));

        // A valid header without frames cannot be embedded.
        let empty = tmp.path().join("empty.wav");
        std::fs::write(
            &empty,
            encode(&WavSpec::new(8_000, 1, SampleFormat::Int16), &[]).bytes,
        )
        .expect("write");
        assert!(prep.prepare(&empty, None).is_err());
    }
}
