# Evaluation

Search quality is the product. Every analysis or ranking change is measured.

## Data tiers
| Tier | Source | In git? | Use |
|---|---|---|---|
| **Fixtures** | `sonar fixtures` (synthetic, seeded) | Generator only | Exact truth for DSP (BPM, key, pitch, loop/one-shot, envelope). Runs in `check.sh`. |
| **Silver** | Real files whose names carry labels (`..._124bpm_Fmin_...`, `Kick_...`) | No | Large, noisy labels for BPM/key/category. Filename is hidden from the analyzer during eval. |
| **Golden** | ~300 hand-labeled real samples from the founder's library (`SONAR_GOLDEN_DIR`) | No (only `eval/golden/*.example.*` templates) | Final authority for category, similarity and text queries. |

Golden files (local, in `$SONAR_GOLDEN_DIR`):
- `labels.csv`: `path,category,kind,bpm,key,note,notes` (empty = unknown)
- `similar.jsonl`: `{"query": "path", "relevant": ["path", ...]}`
- `text_queries.jsonl`: `{"q": "dark metallic kick", "relevant": ["path", ...]}`

Templates: `eval/golden/*.example.*`.

## Fixture manifest
`sonar fixtures <dir> [--seed N] [--skip-long]` writes 135 files (~105 MB,
~5.5 s in a debug build) into `tonal/ chords/ loops/ oneshots/ noise/ edge/
formats/ containers/ corrupt/` plus `<dir>/fixtures.json`. All groups but
`containers/` are WAV. `containers/` (20 files, ~1 MB) covers every other
scanner extension with hand-written, dependency-free writers: AIFF (`.aif`
and `.aiff`), FLAC (RFC 9639), ALAC in `.m4a` (ISO/IEC 14496-12), AAC-LC in
`.m4a` (ISO/IEC 14496-3 raw access units, `esds`, 2112 frames of priming),
MP3 (ISO/IEC 11172-3 / 13818-3 frames + Xing/Info + LAME tag with encoder
delay/padding) and Ogg Vorbis (Vorbis I + RFC 3533, end-trimming granule
positions). Lossless ones carry real content and full truth; **lossy ones
are digital silence** (`format.lossless: false`): sample-level truth does
not apply, they check container parsing, rate/channels and exact gapless
length only. The one exception is the AAC burst file: silence plus a single
hand-encoded MDCT coefficient (bin 16, ≈ 355 Hz, one access unit) whose
window starts at `truth.burst_start_frame` = 3008 of the playable stream (access
unit 5 = decoded frame 5120, minus the 2112 priming), which proves priming
is trimmed from the *start*: the first non-zero decoded sample must be at
the onset (±64 frames for a 1 % threshold on the rising sine window; an
untrimmed decoder is 2112 late). The four AAC files cover priming given as
`elst` + `iTunSMPB`, `elst` only and `iTunSMPB` only (declared lengths are
not multiples of 1024, so end padding is non-trivial). Cross-checked with
Apple `afinfo`/`afconvert`: valid/priming/remainder frames and the burst
(first non-zero sample 3008, peak 0.25) match exactly, except that Apple
ignores the end of an edit list (the `elst`-only file decodes to 13,248
frames there, `segment_duration` honoured here per ISO/IEC 14496-12 and as
ffmpeg does). Known gap: symphonia 0.6.1 ignores both tags; `decode::gapless`
reads them itself and a test pins the upstream behaviour. File names are neutral (`loops/loop_006.wav`,
`containers/clip_009.mp3`); **truth lives only in the manifest**. Same seed +
same `sonar-core` version ⇒ byte-identical output on the same
platform/toolchain (hand-rolled SplitMix64, one stream per file path,
sorted output); across OSes libm `sin`/`exp`/`powf` may differ in the last
bit, which can flip a quantised sample, but truth values are unaffected.
The seed only changes noise content (noise files, snare/hat noise, random
corrupt bytes). `--seed` is capped at 2^53 − 1 so the manifest's `seed`
stays exact in double-based JSON readers (JavaScript).

Top level: `schema_version` (2), `generator {name, version}`, `seed`,
`fields` (legend: every dotted field path → `description`, `unit`) and
`fixtures` (sorted by `path`). Each entry: `path`, `group`, `description`
(human only — analysis must not read it), `bytes`, `format`, `truth`.
Fields that do not apply are omitted (never `null`).

| Field | Meaning (unit) |
|---|---|
| `format.container` / `codec` | `wav`\|`aiff`\|`flac`\|`mp3`\|`ogg`\|`m4a` / `pcm`\|`flac`\|`alac`\|`mp3`\|`vorbis`\|`aac` |
| `format.lossless` | `true`: decoders must return every int sample `q` as `q/2^(bits−1)` bit-exactly; `false`: lossy, no sample-level truth |
| `format.sample_rate` / `channels` / `bit_depth` | Hz / count / 16, 24, 32 (`bit_depth` absent for lossy) |
| `format.sample_format` / `format_tag` | `int`\|`float` (absent for lossy) / `pcm`\|`ieee_float`\|`extensible` (WAV only) |
| `format.num_frames` / `duration_s` | playable frames (lossy: after gapless removal of encoder delay/padding) / s; `format` is absent for corrupt files |
| `truth.kind` | `tone` (sustained pitched), `one_shot`, `loop`, `noise`, `silence` |
| `truth.category` | `kick`, `snare`, `hat_closed`, `hat_open`, `drums`, `click`, `synth`, `sine`, `noise` |
| `truth.bpm` / `bars` / `beats_per_bar` | BPM / bars / quarter-note beats; loop length = `bars·beats_per_bar·60/bpm` s, rounded to whole frames |
| `truth.key {tonic, tonic_pc, mode}` | tonic name, pitch class 0 = C, `major`\|`minor` |
| `truth.chords[] {symbol, root, quality, roman, start_s, duration_s}` | e.g. `F#m`, `F#`, `minor`, `i`, s, s |
| `truth.note_midi` / `note_name` / `pitch_hz` | MIDI (69 = A4) / `C4` = MIDI 60 / `440·2^((m−69)/12)` Hz |
| `truth.noise_color` | `white` \| `pink` (Paul Kellet filter) |
| `truth.peak_dbfs` / `rms_dbfs` | dBFS measured on the **written** samples (ints scaled by `1/2^(bits−1)`; full-scale sine RMS = −3.01 dBFS); absent for digital silence |
| `truth.is_silent` | every sample is exactly 0 |
| `truth.burst_start_frame` | AAC burst file only: playable-stream frame where the burst window starts (frames) |
| `truth.corrupt {reason, expect}` | defect id (e.g. `truncated_header`) and required decoder behaviour |

Spelling: one pitch-class table everywhere — `C C# D Eb E F F# G Ab A Bb
B`; notes use scientific pitch notation (middle C = `C4`). Key/chord eval
must compare pitch classes (`tonic_pc`), not strings.

Corrupt `expect`: `error` — the decoder must return an error;
`error_or_partial` — an error, or a partial/full decode, is acceptable. In
both cases it must not panic, hang or allocate unboundedly (one file has a
`LIST` chunk claiming ~4 GiB).

## Metrics
| Area | Metric | Target |
|---|---|---|
| Loop/one-shot | accuracy | ≥ 95 % |
| BPM (loops) | acc ±1 BPM; octave-error rate reported separately | ≥ 90 % |
| Key | exact + MIREX weighted score | ≥ 80 % weighted |
| Note (tonal one-shots) | exact pitch class / exact MIDI | ≥ 90 % |
| Category | top-1 accuracy, macro-F1, confusion matrix | ≥ 90 % top-1 |
| Similarity | P@10, MRR | P@10 ≥ 0.7 |
| Text / NL search | NDCG@10, recall@50 | track, no regressions |
| Latency | search p50/p95 at 500k docs | p95 < 50 ms |
| Throughput | scan files/s, analysis files/s | ≥ 2,000 / ≥ 100 |

## Running
```
sonar eval --fixtures            # always; part of check.sh
sonar eval --silver <dir>        # real files with labeled names
sonar eval --golden              # uses $SONAR_GOLDEN_DIR
```
Reports: `eval/reports/<date>-<gitsha>.{json,md}` (gitignored except
`eval/reports/BASELINE.md`, which records the current accepted numbers).

## Regression rule
A change that lowers any tracked metric by more than 1 point (or raises p95
latency by > 10 %) must be reverted or explicitly accepted with an ADR.
`sonar eval --fixtures --check` enforces this for fixtures inside
`scripts/check.sh` (non-zero exit on regression). BASELINE.md therefore
needs a machine-readable block (e.g. a fenced JSON section) next to the
human-readable table.
