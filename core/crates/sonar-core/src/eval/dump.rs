//! `sonar eval --category-dump <file>`: per-file category evidence as JSON
//! lines, for tuning and auditing the classifier (ADR-028). First line:
//! `{"prompts": [[category, prompt], ...]}` (empty without CLAP); then one
//! object per analysed, labeled file: `id`, `truth`, `kind` label, the
//! [`crate::classify::DspCues`], the DSP-only and shipped decisions and
//! (with CLAP) the cosine to every prompt in the first line's order.

use std::io::Write;
use std::path::Path;

use crate::classify::PromptBank;
use crate::{Error, Result};

use super::dataset::Item;
use super::run::Analysed;

/// Writes the dump (see the module docs).
///
/// # Errors
/// The file cannot be written.
pub(crate) fn write(
    path: &Path,
    items: &[Item],
    analysed: &[std::result::Result<Analysed, String>],
    bank: Option<&PromptBank>,
) -> Result<()> {
    let io = |source| Error::Io {
        path: path.to_path_buf(),
        source,
    };
    let mut out = std::io::BufWriter::new(std::fs::File::create(path).map_err(io)?);
    let prompts: Vec<(String, &str)> = bank
        .map(|b| {
            b.prompt_scores(&[0.0; crate::embed::EMBED_DIM])
                .into_iter()
                .map(|(c, p, _)| (c.as_str().to_string(), p))
                .collect()
        })
        .unwrap_or_default();
    writeln!(out, "{}", serde_json::json!({ "prompts": prompts })).map_err(io)?;
    for (item, r) in items.iter().zip(analysed) {
        let (Some(truth), Ok(a)) = (item.labels.category, r) else {
            continue;
        };
        let c = a.obs.category;
        let line = serde_json::json!({
            "id": item.id,
            "truth": truth,
            "kind": item.labels.kind,
            "cues": a.cues,
            "dsp": c.and_then(|c| c.dsp),
            "fused": c.and_then(|c| c.fused),
            "prompts": a.prompts,
        });
        writeln!(out, "{line}").map_err(io)?;
    }
    out.flush().map_err(io)
}
