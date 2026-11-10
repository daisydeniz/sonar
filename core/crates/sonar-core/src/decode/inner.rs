//! One file through the decoder: open, choose how the length is found
//! (header for a length-only probe of intact PCM, ADR-015, else the packet
//! loop), resample the analysis window and assemble the result.

use std::io;
use std::path::Path;
use std::sync::atomic::Ordering;

use super::source::{self, decode_err};
use super::{
    analysis_window, header, stream, DecodeOptions, Decoded, Decoder, SourceInfo, Window,
    MAX_RESERVE_FRAMES,
};
use crate::meta::channels::channel_weights;
use crate::meta::{BasicMeta, MetaOptions};
use crate::{Error, Result};

impl Decoder {
    /// Unguarded body of [`Decoder::decode`] / [`Decoder::decode_with_meta`].
    /// `allow_header`: a length-only probe may take the length from the
    /// container header ([`header`]); only tests pass `false`.
    pub(super) fn decode_inner(
        &mut self,
        path: &Path,
        opts: &DecodeOptions,
        meta: Option<&MetaOptions>,
        allow_header: bool,
    ) -> Result<(Decoded, Option<BasicMeta>)> {
        let target = opts.target_rate;
        if !(source::MIN_SAMPLE_RATE..=source::MAX_SAMPLE_RATE).contains(&target) {
            return Err(decode_err(path, format!("invalid target rate {target} Hz")));
        }
        let mut src = source::open(path)?;
        let rate = src.sample_rate;
        let block = self
            .resampler
            .input_block(rate, target)
            .map_err(|e| decode_err(path, e))?;
        let window = analysis_window(opts.max_analysis_secs, rate, target, block);

        self.mono.clear();
        // The header is untrusted: reserve at most the window and 2 Mi
        // frames (8 MiB); longer windows grow amortised.
        let declared = src.declared_frames.unwrap_or(0);
        let reserve = usize::try_from(declared)
            .unwrap_or(usize::MAX)
            .min(window.keep_frames)
            .min(MAX_RESERVE_FRAMES);
        self.mono.reserve(reserve);
        if let Some(m) = meta {
            let weights = channel_weights(src.layout.as_ref(), src.channels);
            self.meter.reset(rate, &weights, m.max_frames(rate));
        }
        let mut source = SourceInfo {
            container: std::mem::take(&mut src.container),
            codec: std::mem::take(&mut src.codec),
            sample_rate: rate,
            channels: u16::try_from(src.channels).unwrap_or(u16::MAX),
            bit_depth: src.bit_depth,
            num_frames: 0,
            declared_frames: src.declared_frames,
            duration_s: 0.0,
        };
        // Length-only probe (meta stage): nothing to decode or measure.
        let length_only = window.keep_frames == 0 && meta.is_none_or(|m| m.max_frames(rate) == 0);
        let frame_bytes = header::pcm_frame_bytes(&src).filter(|_| allow_header && length_only);
        let outcome = match frame_bytes {
            Some(bytes) => {
                if opts
                    .cancel
                    .as_deref()
                    .is_some_and(|c| c.load(Ordering::Relaxed))
                {
                    return Err(Error::Cancelled);
                }
                match header::verified_frames(src, bytes) {
                    Some(total_frames) => stream::Outcome {
                        total_frames,
                        ..stream::Outcome::default()
                    },
                    // The demuxer was consumed: count on a fresh one.
                    None => {
                        let mut again = source::open(path)?;
                        let same = (again.sample_rate, again.channels, again.declared_frames);
                        if same != (rate, usize::from(source.channels), source.declared_frames) {
                            // Replaced between the two opens: transient, so
                            // an I/O error (the pipeline retries the file).
                            let e = io::Error::other("file changed while reading");
                            return Err(Error::io(path, e));
                        }
                        self.count(path, &mut again, &window, meta, opts)?
                    }
                }
            }
            None => self.count(path, &mut src, &window, meta, opts)?,
        };

        let samples = self
            .resampler
            .resample(&self.mono, rate, target, window.max_out)
            .map_err(|e| decode_err(path, e))?;
        let num_frames = outcome.total_frames;
        // Metering stopped at a bad packet: the measured span is short.
        let meta_partial = outcome.partial || outcome.meter_problem.is_some();
        let meta_problem = outcome.problem.clone().or(outcome.meter_problem.clone());
        source.num_frames = num_frames;
        source.duration_s = num_frames as f64 / f64::from(rate);
        let decoded = Decoded {
            samples,
            sample_rate: target,
            source,
            truncated_for_analysis: num_frames > window.window_frames,
            partial: outcome.partial,
            problem: outcome.problem,
        };
        let meta = meta.map(|_| {
            BasicMeta::from_measurement(&decoded.source, &self.meter, meta_partial, meta_problem)
        });
        Ok((decoded, meta))
    }

    /// The packet loop ([`stream::run`]) over `src`.
    fn count(
        &mut self,
        path: &Path,
        src: &mut source::Opened,
        window: &Window,
        meta: Option<&MetaOptions>,
        opts: &DecodeOptions,
    ) -> Result<stream::Outcome> {
        stream::run(
            path,
            src,
            window.keep_frames,
            &mut self.planar,
            &mut self.mono,
            meta.map(|_| &mut self.meter),
            opts.cancel.as_deref(),
        )
    }
}
