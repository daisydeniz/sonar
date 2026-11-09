//! Turns one catalog entry into file bytes, its manifest `format` and its
//! truth. Level truth (`peak_dbfs`, `rms_dbfs`, `is_silent`) is measured on
//! the samples actually stored (after quantisation).

use super::manifest::{Codec, Container, Format, FormatTag, Kind, SampleKind, Truth};
use super::pcm::Pcm;
use super::render;
use super::rng::SplitMix64;
use super::spec::{Body, Content, FixtureSpec, LossyCodec, Store};
use super::synth::Audio;
use super::wav::{Levels, WavSpec};
use super::{aac, aiff, flac, m4a, mp3, music, ogg, wav};
use crate::{Error, Result};

/// A fixture ready to be written.
#[derive(Debug, Clone)]
pub struct Built {
    /// File bytes.
    pub bytes: Vec<u8>,
    /// Format as written; `None` for corrupt files.
    pub format: Option<Format>,
    /// Truth values.
    pub truth: Truth,
}

/// Builds `spec` with the PRNG stream of `(seed, spec.path)`.
pub fn build(spec: &FixtureSpec, seed: u64) -> Result<Built> {
    let mut rng = SplitMix64::for_stream(seed, &spec.path);
    match &spec.body {
        Body::Audio {
            spec: ws,
            content,
            peak_dbfs,
            store,
        } => {
            let (audio, mut truth) = rendered(content, ws, *peak_dbfs, &mut rng);
            let lossless = |enc: fn(&Pcm) -> Vec<u8>, container, codec| {
                let pcm = Pcm::quantize(ws, &audio.interleave());
                (enc(&pcm), pcm.levels(), pcm_format(&pcm, container, codec))
            };
            let (bytes, levels, format) = match store {
                Store::Wav => {
                    let enc = wav::encode(ws, &audio.interleave());
                    (enc.bytes, enc.levels, wav_format(ws, enc.num_frames))
                }
                Store::Aiff => lossless(aiff::encode, Container::Aiff, Codec::Pcm),
                Store::Flac => lossless(flac::encode, Container::Flac, Codec::Flac),
                Store::Alac => lossless(m4a::encode, Container::M4a, Codec::Alac),
            };
            set_levels(&mut truth, &levels);
            Ok(Built {
                bytes,
                format: Some(format),
                truth,
            })
        }
        Body::LossySilence {
            codec,
            sample_rate,
            channels,
            frames,
        } => {
            let (bytes, container, c) = match codec {
                LossyCodec::Mp3 => (
                    mp3::encode_silence(*sample_rate, *channels, *frames),
                    Container::Mp3,
                    Codec::Mp3,
                ),
                LossyCodec::Vorbis => (
                    ogg::encode_silence(*sample_rate, *channels, *frames),
                    Container::Ogg,
                    Codec::Vorbis,
                ),
            };
            let bytes = bytes.ok_or_else(|| Error::UnsupportedFormat {
                path: spec.path.clone().into(),
                reason: format!("{c:?} writer: {sample_rate} Hz, {channels} ch unsupported"),
            })?;
            let truth = Truth {
                kind: Some(Kind::Silence),
                is_silent: Some(true),
                ..Truth::default()
            };
            let format = Format {
                container,
                codec: c,
                lossless: false,
                sample_rate: *sample_rate,
                channels: *channels,
                bit_depth: None,
                sample_format: None,
                format_tag: None,
                num_frames: *frames,
                duration_s: music::round6(*frames as f64 / f64::from(*sample_rate)),
            };
            Ok(Built {
                bytes,
                format: Some(format),
                truth,
            })
        }
        Body::Aac(aac_spec) => {
            let bytes = aac::encode(aac_spec).ok_or_else(|| Error::UnsupportedFormat {
                path: spec.path.clone().into(),
                reason: format!("AAC writer: unsupported {aac_spec:?}"),
            })?;
            let burst = aac_spec.burst_start_frame();
            let truth = Truth {
                kind: burst.is_none().then_some(Kind::Silence),
                is_silent: Some(burst.is_none()),
                burst_start_frame: burst,
                ..Truth::default()
            };
            let format = Format {
                container: Container::M4a,
                codec: Codec::Aac,
                lossless: false,
                sample_rate: aac_spec.sample_rate,
                channels: aac_spec.channels,
                bit_depth: None,
                sample_format: None,
                format_tag: None,
                num_frames: aac_spec.frames,
                duration_s: music::round6(aac_spec.frames as f64 / f64::from(aac_spec.sample_rate)),
            };
            Ok(Built {
                bytes,
                format: Some(format),
                truth,
            })
        }
        Body::Corrupt(kind) => Ok(Built {
            bytes: kind.build(&mut rng),
            format: None,
            truth: Truth {
                corrupt: Some(kind.truth()),
                ..Truth::default()
            },
        }),
    }
}

/// The integer samples a lossless entry stores: `Some` for AIFF, FLAC,
/// ALAC and integer WAV; `None` for float WAV, lossy and corrupt entries.
pub fn stored_pcm(spec: &FixtureSpec, seed: u64) -> Option<Pcm> {
    let Body::Audio {
        spec: ws,
        content,
        peak_dbfs,
        store,
    } = &spec.body
    else {
        return None;
    };
    if *store == Store::Wav && ws.sample_format.is_float() {
        return None;
    }
    let mut rng = SplitMix64::for_stream(seed, &spec.path);
    let (audio, _) = rendered(content, ws, *peak_dbfs, &mut rng);
    Some(Pcm::quantize(ws, &audio.interleave()))
}

fn rendered(
    content: &Content,
    spec: &WavSpec,
    peak_dbfs: Option<f64>,
    rng: &mut SplitMix64,
) -> (Audio, Truth) {
    let (mut audio, truth) = render::render(content, spec, rng);
    if let Some(p) = peak_dbfs {
        audio.normalize_peak(p);
    }
    (audio, truth)
}

fn set_levels(truth: &mut Truth, levels: &Levels) {
    truth.is_silent = Some(levels.is_silent());
    truth.peak_dbfs = levels.peak_dbfs().map(round3);
    truth.rms_dbfs = levels.rms_dbfs().map(round3);
}

fn wav_format(spec: &WavSpec, num_frames: u64) -> Format {
    let float = spec.sample_format.is_float();
    let format_tag = if spec.extensible {
        FormatTag::Extensible
    } else if float {
        FormatTag::IeeeFloat
    } else {
        FormatTag::Pcm
    };
    Format {
        container: Container::Wav,
        codec: Codec::Pcm,
        lossless: true,
        sample_rate: spec.sample_rate,
        channels: spec.channels,
        bit_depth: Some(spec.sample_format.bits()),
        sample_format: Some(if float {
            SampleKind::Float
        } else {
            SampleKind::Int
        }),
        format_tag: Some(format_tag),
        num_frames,
        duration_s: music::round6(num_frames as f64 / f64::from(spec.sample_rate)),
    }
}

fn pcm_format(pcm: &Pcm, container: Container, codec: Codec) -> Format {
    let num_frames = pcm.frames() as u64;
    Format {
        container,
        codec,
        lossless: true,
        sample_rate: pcm.sample_rate,
        channels: pcm.channels,
        bit_depth: Some(pcm.bits),
        sample_format: Some(SampleKind::Int),
        format_tag: None,
        num_frames,
        duration_s: music::round6(num_frames as f64 / f64::from(pcm.sample_rate)),
    }
}

/// Rounds to 3 decimals (0.001 dB) for stable, readable manifest values.
/// `-0.0` is normalised to `0.0` so the manifest never prints `-0.0`.
fn round3(x: f64) -> f64 {
    let r = (x * 1e3).round() / 1e3;
    if r == 0.0 {
        0.0
    } else {
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::catalog::catalog;
    use crate::fixtures::manifest::Group;

    #[test]
    fn round3_never_yields_negative_zero() {
        let r = round3(-0.0003);
        assert_eq!(r, 0.0);
        assert!(r.is_sign_positive());
        assert_eq!(round3(-1.23456), -1.235);
    }

    #[test]
    fn stored_pcm_matches_built_levels_and_format() {
        for spec in catalog(false)
            .iter()
            .filter(|f| f.group == Group::Containers)
        {
            let built = build(spec, 1).unwrap();
            let fmt = built.format.unwrap();
            match stored_pcm(spec, 1) {
                Some(pcm) => {
                    assert!(fmt.lossless, "{}", spec.path);
                    assert_eq!(pcm.frames() as u64, fmt.num_frames, "{}", spec.path);
                    assert_eq!(Some(pcm.bits), fmt.bit_depth, "{}", spec.path);
                    let peak = pcm.levels().peak_dbfs().map(round3);
                    assert_eq!(peak, built.truth.peak_dbfs, "{}", spec.path);
                }
                None => {
                    assert!(!fmt.lossless, "{}", spec.path);
                    // Lossy: digital silence, except a burst file.
                    let silent = built.truth.burst_start_frame.is_none();
                    assert_eq!(built.truth.is_silent, Some(silent), "{}", spec.path);
                }
            }
        }
    }
}
