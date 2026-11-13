//! `sonar wave`: waveform peaks (and the preview PCM summary) of a library
//! file, the CLI side of `SonarEngine.waveformPeaks` / `previewPcm`.

use anyhow::{Context, Result};
use serde_json::json;

use crate::user_cmd::{open, resolve_one};

/// Prints `buckets` min/max pairs of `file` (an id or a path); with `pcm`
/// also the decoded preview PCM's rate, channels and length.
pub fn run(file: &str, buckets: usize, pcm: bool, as_json: bool) -> Result<()> {
    let engine = open()?;
    let id = resolve_one(&engine, file)?;
    let peaks = engine
        .waveform_peaks(id, buckets)
        .context("computing waveform peaks")?;
    let preview = if pcm {
        Some(engine.preview_pcm(id).context("decoding preview PCM")?)
    } else {
        None
    };
    if as_json {
        let doc = json!({
            "file_id": id,
            "peaks": peaks,
            "pcm": preview.as_ref().map(|p| json!({
                "sample_rate": p.sample_rate,
                "channels": p.channels,
                "samples": p.samples.len(),
                "truncated": p.truncated,
            })),
        });
        println!("{}", serde_json::to_string_pretty(&doc)?);
    } else {
        println!("file #{id}: {} peak buckets", peaks.len());
        for (i, (lo, hi)) in peaks.iter().enumerate() {
            println!("{i:>5}  {lo:>8.4}  {hi:>8.4}");
        }
        if let Some(p) = preview {
            println!(
                "preview PCM: {} Hz, {} ch, {} samples{}",
                p.sample_rate,
                p.channels,
                p.samples.len(),
                if p.truncated { " (truncated)" } else { "" }
            );
        }
    }
    Ok(())
}
