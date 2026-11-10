# Architecture Decision Log

Append new decisions at the bottom. Never delete; supersede instead.
Format: `ADR-NNN — title` · status · context · decision · consequences.

## ADR-001 — macOS first, Windows in ~1 year
Accepted. Founder develops on a MacBook Pro M4 (48 GB). Best UX on Mac is the
goal. Windows is likely later, so platform-neutral logic must be portable.

## ADR-002 — Rust core + Swift (SwiftUI/AppKit) UI, bridged with UniFFI
Accepted. Rust holds all product logic (scan, DSP, index, search) and is
reused on Windows. Swift gives native UX, AVAudioEngine preview, native drag &
drop. Rejected: Tauri (webview list/preview limits for "best UX"), pure Swift
(Windows rewrite of the valuable part), JUCE app (slow UI dev, C++ risk).

## ADR-003 — Direct distribution, not Mac App Store
Accepted. App Store sandbox blocks v2 plugin hosting (plugins need ~/Library,
licenses, iLok). Notarized DMG + Paddle or Lemon Squeezy (merchant of record).
Hardened runtime, no sandbox.

## ADR-004 — Permissive dependencies only
Accepted. Closed-source commercial product → MIT/Apache/BSD/ISC/Zlib/MPL-2.0
(file-level) only. No GPL/AGPL (aubio, Essentia, libKeyFinder). Enforced with
`cargo deny`. ML model weights must also allow commercial redistribution;
record model + license in this log before shipping.
Amendment (T0.2): also allowed `Apache-2.0 WITH LLVM-exception` and
`Unicode-3.0` (Unicode data tables, e.g. `unicode-ident`). Allow list lives in
`core/deny.toml`; any new license needs an amendment here first.

## ADR-005 — MVP scope: Sononym parity + natural-language search
Accepted. Preset search moves to v2. See docs/PRODUCT.md.

## ADR-006 — Local CLAP embeddings via ONNX Runtime
Accepted (model choice pending, see M5). Audio+text joint embeddings for
natural-language search and zero-shot categories. Run via `ort` with the
CoreML execution provider on macOS (Neural Engine/GPU), CPU fallback.

## ADR-007 — v2 plugin host = C++/JUCE sidecar process
Accepted for v2. One host for Mac (AU + VST3) and later Windows (VST3).
Built for arm64 and x86_64 (Intel-only plugins under Rosetta). Separate
process for crash isolation. VST2-only and 32-bit plugins unsupported.

## ADR-008 — Quality measured, not asserted
Accepted. Every analysis/search change runs `sonar eval` against synthetic
fixtures and (when present) the golden set; regressions block merge.

## ADR-009 — Scanner file identity and walk policy (T1.2)
Accepted. Identity = `size` + `mtime` (ns) + `quick_hash`, where
`quick_hash` = XXH3-64 (`twox-hash`, MIT) over the size as LE u64 + first
64 KiB + last 64 KiB (whole file if ≤ 128 KiB), stored bit-cast as INTEGER.
Unchanged size+mtime → the file is not opened. Moves/renames are matched by
(size, quick_hash) within one library, so ids and user data survive.
Changing the recipe invalidates every stored hash (needs a migration that
rehashes). Symlinks are **not** followed (no loops, no double-indexing);
dot-files/dirs and AppleDouble `._*` are skipped. Missing files become
`status='missing'` (rows and user data kept); an offline root or an
unreadable folder never marks rows missing. Revisit symlink support if users
keep packs behind links (follow with a visited-inode set).

## ADR-010 — Decode policy (T1.3)
Accepted. `sonar_core::decode` uses symphonia 0.6.1 (MPL-2.0; wav, aiff,
pcm, adpcm, flac, mp3, ogg/vorbis, isomp4/aac/alac, id3v2) and rubato 5.0.1
`Fft` resampler (MIT/Apache; chunk 4096, 4 sub-chunks, delay trimmed and
tail flushed → `ceil(n·22050/src)` samples, ±1-sample alignment). Analysis
input = mean-of-channels mono f32 at 22.05 kHz, capped at 30 s (+ an edge
margin of max(50 ms, 2 resampler blocks) decoded past the cap). Duration is
**counted**: decoded frames in the window + packet durations past it (raw
ticks, converted once); counted < declared → `partial`. Policy: a packet
the codec rejects → silence of equal length + `partial` (> 32 in a row
stops); malformed container mid-stream → `partial`; real I/O error (not EOF)
→ `Error::Io` (offline drive must not be stored as a damaged file); nothing
decoded + error → error; valid header with 0 frames → Ok, empty; 0-byte file
→ error. Hard caps: 1 kHz–768 kHz, 1–32 ch, ≤ 2^18 frames/packet, ≤ 2^23
packets, ≤ 64 Mi source frames buffered. Every call runs under
`catch_unwind` (symphonia-format-riff 0.6.1 has a u16 overflow panic on a
crafted WAV header in debug builds).

## ADR-011 — Model mix for the agent team (usage budget)
Accepted (2026-11-09). Founder is on the Pro plan; usage limits, not model
quality, were the bottleneck (1–4 tasks per 5-hour window). Orchestrator
(autopilot `--model opus`), reviewer, dsp-analysis and search-ranking stay on
Opus — they own correctness and the product's differentiators. rust-core,
swift-ui, eval and architect run on Sonnet (well-specified engineering under
the same gates: fmt, clippy, tests, cargo deny, eval, Opus review). If review
finds repeated quality issues from a Sonnet agent, move it back to Opus and
note it here.

## ADR-012 — Basic meta: peak and loudness definitions (T1.4)
Accepted. `sonar_core::meta` measures in the decode pass
(`Decoder::decode_with_meta`; decode-only API unchanged): packets are
decoded up to max(analysis window, meta window), only the analysis window is
buffered (bit-identical to `decode`), the meter sees native-rate planar
samples of all channels before downmix, gapless priming/padding excluded.
A codec error on a packet decoded only for the meter stops metering and the
loop goes back to counting (same `partial`/`problem` as `decode`;
`BasicMeta.partial` set). Known difference: if a *truncated final packet*
lies past the analysis window, `decode` counts its declared duration while
metering decodes it and reports the frames actually present (fixture
`corrupt/bad_004.wav` with a 0 s window: 11,520 counted vs 11,025 decoded) —
partial files only, always shorter, < 1 packet; metering is the more
accurate of the two.
Meta window `MetaOptions.max_meta_secs` = 600 s; duration still counts the
whole file, `measured_s` reports the span. **Peak** = sample peak (no true
peak), max |x| over channels, dBFS (ints / 2^(bits−1)). **Loudness** =
ITU-R BS.1770-4 integrated: K-weighting for any rate from the analogue
prototypes via the bilinear transform (De Man's derivation, as in
libebur128/pyloudnorm, both MIT; written from the formulas), direct form I
in f64; RLB high-pass numerator literally 1, −2, 1 at every rate, like
libebur128/pyloudnorm/ffmpeg so they can serve as eval references (its
pass-band gain is rate-dependent: +0.05 LU at 22.05 kHz, −0.02 LU at 96 kHz
vs 48 kHz, inside ±0.1). 400 ms blocks, 75 % overlap (100 ms steps =
round(fs/10) frames), gates −70 LUFS and −10 LU. Weights: L/R/C 1.0, side
(or rear when no side pair, i.e. 5.1) 1.41, LFE excluded, unknown layouts
1.0; mono = one channel, G = 1 (no dual-mono +3 dB). A positioned mask
equal to the count-derived WAVE default (`from_wave_channel_count(n)`, what
symphonia gives plain multichannel WAVs) counts as unknown → all 1.0, except
n = 6 (= canonical 5.1); otherwise channel 4 of a multitrack/ambisonic WAV
would be dropped as "LFE" (cost: an explicit 3.1/4.1 mask is weighted 1.0
too). Spans < 400 ms: one block over the whole span
(approximation). Silence / below gate / nothing measured → `None` (NULL in
`audio_meta`), never −∞/NaN; non-finite float samples are ignored. Store:
schema 0001 `audio_meta` as is (no migration); `measured_s`/`partial` are
not stored (partial goes to `files.status/error` in the pipeline, T1.5).

## ADR-013 — Job pipeline (T1.5)
Accepted. `sonar_core::jobs` runs the analysis stages in the background.
- **Stages + versions.** `Stage {Meta, Dsp, Embed}`, each with a `VERSION`
  const. Progress is the `file_stages(file_id, stage, version, outcome,
  message, done_at)` table (migration 0002), not `files.status`. A file is
  *pending* for a stage with no row or `version < current`; bumping a const
  re-runs just that stage. Changed content (scan) deletes the file's rows;
  moves keep them; a restored file loses its `error` rows (retried).
  `Embed` is declared but not runnable until T5.2 (skipped explicitly).
- **Staged priority** across the whole store: every pending `Meta` file
  before the first `Dsp` file; ids ascending within a stage. Eligible: online
  library, status new/meta/analyzed (plus an `error` row at an outdated
  version, so a bump retries failures); `missing` and scan-level `error`
  rows never.
- **Meta stage** probes with 0 s analysis and 0 s meta windows (no buffer,
  no meter): format + counted duration (header length for intact PCM
  WAV/AIFF, ADR-015); `audio_meta` peak/loudness stay
  NULL (format-only upsert, so a re-run never erases them). **Dsp stage**
  decodes the 30 s analysis window and meters peak/loudness over the 600 s
  default window (ADR-012). `BasicMeta.problem` carries why a result is
  partial into `file_stages.message`.
- **Atomicity.** One coordinator thread owns the connection; rayon workers
  only decode. Each batch (default 128 files) is committed in a single
  `IMMEDIATE` transaction (`audio_meta` + `file_stages` + `files`), so
  `kill -9` leaves whole batches; `PRIMARY KEY (file_id, stage)` plus the
  pending query make resume duplicate-free.
- **Errors.** Decode/format error → `error` row + `files.status='error'`
  (retried only after a content change or version bump); damaged but readable
  → `partial` (message = problem); `Error::Io` (offline drive, permissions)
  → nothing recorded, file stays pending, counted as skipped; a worker panic
  → `error` row "internal error". Panic messages are silenced inside
  guarded sections by a once-installed hook (`quiet_panic`).
- **Stale claims.** `(size, mtime)` is captured at claim; the commit applies
  a result only if the row still matches and is not `missing`, else it is
  dropped (the scan already reset the file).
- **Threading/control.** `jobs::start` opens its own connection and spawns
  the job thread; `JobHandle` pause/resume (Condvar, no busy loop, between
  files), cancel (stop claiming, in-flight files finish and commit), polled
  `progress()` and an optional event callback.

## ADR-014 — File watching (T1.6)
- **Strategy**: `notify` 8.2.2 (FSEvents backend, `macos_fsevent` only, no
  crossbeam) events only *trigger* an incremental `scan_library_with` of the
  affected library; no per-event DB edits (moves need the library-wide view,
  the size+mtime fast path makes a no-op rescan cheap). Events on dot-files
  and non-audio files are dropped; directory events, renames, unknown kinds,
  `need_rescan` and watcher errors always trigger.
- **Debounce**: per library, 400 ms of silence, at most 5 s after the first
  event of a burst (pure state machine, unit-tested without real time).
  Measured write-to-`Scanned` latency with a 100 ms debounce: ~120 ms.
- **Serialization**: one watcher thread owns its `Store` connection and runs
  scans one at a time, so scans of a library never overlap. A failed scan
  (e.g. busy beyond the 5 s timeout) is reported and retried after 5 s.
  Job-pipeline commits are short batches in WAL; the pipeline's stale-claim
  check discards results for files a scan reset.
- **Offline**: a 2 s poll checks each root. Missing root: drop the notify
  watch (cannot watch a missing path), scan once (`online=0`, rows kept).
  Back: new watch, scan (`online=1`). A library offline in the index but
  present on disk is rescanned each poll. The same poll adopts libraries
  added to the store. Every library is scanned once at start.
- **Robustness**: callback and scan run under `quiet_panic::guarded`;
  panics become `WatchEvent::Error`.
- **License**: notify is CC0-1.0 (public-domain dedication); allowed in
  `deny.toml` as a `[[licenses.exceptions]]` entry for `notify` only.
- **Spacing**: after a scan, event-driven rescans of that library wait
  `2 x scan time` besides the 400 ms quiet period (scan duty cycle ≤ ~1/3
  during long copies; ms-fast libraries still react in ~0.4 s); root changes, manual rescans and retries skip it.
  A root that exists but is offline in the index is retried every 5 s.
- **Stale mount point**: the scanner treats a root that is an empty
  directory while the library has present rows as offline (`online=0`, no
  row marked missing), so an unmounted volume's leftover mount point cannot
  wipe the index. Emptying a library on purpose keeps it "offline" until an
  entry appears.
- **Shutdown**: `stop()` from inside the callback only requests the stop
  (no self-join). A watcher-thread panic outside scan/callback emits a final
  `Error{library_id: None}` and `is_running()` turns false.
- **Limits**: FSEvents does not report changes made remotely on SMB/NAS
  volumes (only the periodic root poll and manual `rescan_now` help there).
  A root replaced by a different volume at the same path within
  one poll interval is treated as unchanged; the 2 s poll bounds
  detection latency for unmounts that raise no event.
- **Measured (T1.7b, `sonar bench watch`, M4 Pro, release, 50k library)**:
  whole-library rescans per burst are cheap enough; no subtree scoping and
  no change to the watcher. Copy only, 10,000 files at 250 files/s (40 s):
  8 scans of 183-212 ms, duty cycle 3 % over the copy, catch-up 0.63 s
  (= 400 ms debounce + one scan). Copy only, 20,000 files unthrottled
  (1.3 s): one 756 ms scan after the burst, catch-up 1.17 s; that scan is
  ~31-43 % of burst + catch-up time, above the "about 1/3" that the 2x
  spacing guarantees for a *sustained* copy (the first scan of a burst is
  not delayed). With the pipeline restarted to run for the whole copy (7
  runs over the 10k copy): all 8 scans overlapped pipeline runs (5.5 s in
  total), scans grew to 553-908 ms, duty cycle 12 %, catch-up 1.9 s, and the
  pipeline slowed 21,481 -> 19,465 files/s (-9 %). 20k unthrottled with the
  pipeline: the copy took 7.1 s instead of 1.3 s (about 5x slower under the
  14-thread pipeline), 2 scans (max 1.7 s), catch-up 3.6 s, pipeline
  20,163 -> 16,903 files/s (-16 %). In all runs 0 `SQLITE_BUSY` watcher
  errors and no pipeline failure; the pipeline's longest gap between progress
  events was 53 ms. Revisit if libraries grow well beyond 50k or scans on slow
  volumes dominate.

## ADR-015 — Header length for the meta probe (T1.7a)
Accepted. Counting packets reads every byte of a PCM file to learn what
its header states; on cold libraries that made the `Meta` stage I/O-bound.
- **Scope**: only a length-only probe (0 s analysis window and 0 s or no
  meter window: the `Meta` stage, `analyze_file`/`sonar meta` with
  `--max-meta-secs 0`, `decode` with `Some(0.0)`). Any non-zero window
  (the `Dsp` stage, `bench decode`) is unchanged, bit for bit.
- **Formats**: WAVE and AIFF/AIFF-C whose codec is uncompressed PCM with one
  frame per block (int 8/16/24/32, float 32/64, A-law, µ-law, incl.
  WAVE_FORMAT_EXTENSIBLE and AIFF-C `sowt`/`fl32`/`in24`…). Not FLAC (RFC
  9639 STREAMINFO total samples is exact, but only reading the compressed
  frames reveals truncation/corruption, which counting reports as
  `partial`), not MP4 (sample tables, same reason), not ADPCM, MP3, Ogg.
- **Rule** (`decode::header`): symphonia opens the file as before (all
  format fields come from it). The declared length `N` is used only if the
  chunk header just before the payload, re-read from the file, is WAVE
  `data` with `ckSize ≠ 0xFFFFFFFF` or AIFF `SSND` with `offset = blockSize
  = 0`; the payload is a whole number of frames (frame = sample bytes ×
  channels, what the demuxer reads; `nBlockAlign` is used by neither path);
  payload / frame = `N`; and data start + payload ≤ file length. Anything
  else (truncated, oversized declared chunk, unknown/streamed size incl. a
  `fact` length symphonia would otherwise declare, unaligned payload,
  non-zero SSND offset, compressed codec) → reopen and count exactly as
  before. With those rules symphonia's RIFF demuxer would read exactly `N`
  whole frames, so the result equals the counted one; a zero-window probe
  never decodes, so PCM content cannot change it. Tested: header == counted
  (Decoded + BasicMeta) on all 135 fixtures (103 take the header path,
  corrupt ones included in the comparison), truncated/oversized/unaligned/
  streamed WAV and AIFF, payload never read (byte-counting source).
- **No stage version bump**: results are identical to the counted ones, so
  existing `meta` rows stay valid.
- **Known limits** (pre-existing, same on both paths, not changed here):
  - a short final PCM packet is counted at its full 1,152 frames (symphonia
    returns a short read without error), so a WAV cut inside its last
    packet reports up to 1,151 frames too many and is not `partial`; cut
    earlier, it is `partial`;
  - symphonia ignores the AIFF `COMM` `numSampleFrames` and derives the
    length from the `SSND` size, so an AIFF whose `SSND` holds more frames
    than `numSampleFrames` over-reports its length.
- **File replaced between the two opens** (fallback path): reported as
  `Error::Io` ("file changed while reading"), so the pipeline skips and
  retries it instead of recording a permanent error.
- **Measured** (M4 Pro, 14 threads, `bench analyze --stages meta`): cold
  APFS clones (`--cold --clone`) 3k files 5,926 → 33,599 files/s, 20k files
  (13 GB) 1,138 → 37,399 files/s; warm 28,284 → 48,183 (3k) and 28,408 →
  37,166 files/s (20k).
