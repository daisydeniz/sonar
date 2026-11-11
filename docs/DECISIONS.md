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
  `progress()` and an optional event callback. Pause semantics changed in
  the T1.7c addendum below (workers stop starting files instead of blocking).

**Addendum (T1.7c) — batch look-ahead, commit-on-pause.** `sonar bench
analyze` now reports `StageTiming` (pool utilization = busy worker-s /
(threads x wall), coordinator claim / commit / wait time). Measured first
(release, M4 Pro 14 cores, warm cache): with strictly sequential
claim -> compute -> commit the pool idled 36 % (meta) / 14 % (dsp) of the
wall at batch 128 (commit alone 23 % / 7 %, batch tail 0.3 / 1.1 ms per
batch), i.e. > 10 % throughput on the table, so the loop was pipelined:
`jobs/batches.rs` keeps 2 batches in flight (`LOOKAHEAD`), the coordinator
claims ahead and commits finished batches in claim order while workers run
the next. Invariants kept: one `IMMEDIATE` tx per batch, ids claimed once
(monotonic cursor), stale-claim check at commit, BUSY retry, claim-ahead
only inside the current stage (stage drains before the next starts).
Pause/cancel: workers no longer block mid-batch; they stop starting files
(`Unstarted`), the coordinator commits what finished, and after the
pipeline drained dispatches the unstarted claims again (a retry list, in
batch-size chunks, before claiming anything new; dropped on cancel). The
id cursor is never rewound: rayon spreads a batch over threads, so the
unstarted files are not a suffix and a rewind would re-offer files already
committed (skipped/stale ones stay pending) and double count them.
Unstarted files never got a row: no duplicates, kill -9 safe as before. Files/s, before -> after, (utilization):

| stage | files | batch | before | after |
|---|---|---|---|---|
| meta | 3k | 32 | 37,783 (47 %) | 48,631 (62 %) |
| meta | 3k | 128 | 40,565 (54 %) | 56,233 (71 %) |
| meta | 3k | 512 | 49,409 (64 %) | 60,910 (74 %) |
| meta | 20k | 128 | 36,501 (64 %) | 43,219 (79 %) |
| meta | 20k | 512 | 40,704 (71 %) | 45,860 (85 %) |
| meta, cold clone | 20k | 128 | 37,009 (69 %) | 42,755 (81 %) |
| dsp | 3k | 32 | 6,874 (74 %) | 7,845 (83 %) |
| dsp | 3k | 128 | 7,932 (84 %) | 8,026 (90 %) |
| dsp | 3k | 512 | 8,059 (90 %) | 8,507 (91 %) |
| dsp | 20k | 128 | 6,820 (86 %) | 7,975 (90 %) |
| dsp | 20k | 512 | 7,127 (89 %) | 8,461 (93 %) |

Single runs, run-to-run noise about +-5 %; the gain is largest at small
batches (the 128 default now costs little vs 512). Commit stays 7-9 % (dsp)
of wall but overlaps compute; the remaining dsp idle is the coordinator
thread competing for a core and rayon spawn overhead.

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
- **Known limits** (pre-existing, same on both paths, not changed here;
  both fixed by T1.7d, see the addendum):
  - a short final PCM packet is counted at its full 1,152 frames (symphonia
    returns a short read without error), so a WAV cut inside its last
    packet reports up to 1,151 frames too many and is not `partial`; cut
    earlier, it is `partial`;
  - symphonia ignores the AIFF `COMM` `numSampleFrames` and derives the
    length from the `SSND` size, so an AIFF whose `SSND` holds more frames
    than `numSampleFrames` over-reports its length.
- **Addendum T1.7d — exact PCM length** (`decode::stream::packet_ticks`,
  `decode::aiff`):
  - PCM in WAVE/AIFF is counted by the bytes each packet holds
    (`data.len() / (sample bytes × channels)`), not `Packet::dur`; the PCM
    decoder decodes exactly those whole frames, so counted = decoded and a
    trailing partial frame is dropped on both. A truncated WAV reports
    exactly `floor(surviving data bytes / frame)` frames and is `partial`.
  - AIFF: `COMM` `numSampleFrames` is the declared length (AIFF 1.3: the
    number of sample frames in `SSND`). `SSND` longer → capped at `COMM`
    (decode, meter and length; trimmed, not `partial`; packets past `COMM`
    are still read and counted, the total is capped afterwards; counted,
    not the header path). `SSND` shorter → `partial` (also for an intact
    file whose writer over-declared `COMM`; watch AIFF `partial` counts in
    eval). `numSampleFrames = 0` with a non-empty `SSND` = never finalised
    → `SSND` size used.
  - Truncated AIFF: symphonia 0.6.1 cannot open a `FORM` cut by the end of
    the file (it skips the `SSND` body while scanning chunks). So before
    symphonia probes, `source::open` runs `aiff::prepare` on the same file
    handle: a bounded seek-only chunk walk (≤ 4,096 headers, stops once
    `COMM` and `SSND` are found). Whenever the `FORM` extends past the end
    of the file (no symphonia failure needed), the demuxer reads the file
    through an in-memory header overlay: a cut `SSND` keeps its
    surviving bytes, any other cut chunk is dropped by ending the `FORM`
    before it. One open and one code path for every AIFF, not a
    fail-then-retry. Declared length stays `COMM`, so the result is the exact
    surviving frames + `partial` (not `partial` when only chunks after the
    sound data were lost). The file is never written.
  - Header path unchanged in rules; it still equals counting on every
    fixture and on the truncated/over-long cases above.
  - **Stage versions**: `meta` 1 → 2 and `dsp` 1 → 2 (both store
    `num_frames`; `dsp` also changes peak/loudness of over-long AIFF and
    turns truncated-AIFF errors into partial results). Every file re-runs
    both stages once; the header path keeps the `meta` re-run cheap.
  - Cost: every file pays one unbuffered 12-byte read + rewind (form
    check) before probing; AIFF adds a few header reads/seeks on the same
    handle (like the MP4 `gapless` walk); the overlay only exists for
    `FORM`s cut by the end of the file. Measured with `sonar bench analyze`
    (STATUS log, T1.7d).
- **File replaced between the two opens** (fallback path): reported as
  `Error::Io` ("file changed while reading"), so the pipeline skips and
  retries it instead of recording a permanent error.
- **Measured** (M4 Pro, 14 threads, `bench analyze --stages meta`): cold
  APFS clones (`--cold --clone`) 3k files 5,926 → 33,599 files/s, 20k files
  (13 GB) 1,138 → 37,399 files/s; warm 28,284 → 48,183 (3k) and 28,408 →
  37,166 files/s (20k).

## ADR-016 — Text analysis chain and filename priors (T2.1)
Accepted. `sonar_core::text` is the one analysis chain for names, paths and
queries (index and query must agree); T2.2 wraps it in a Tantivy tokenizer.
- **Chain**: split on separators, letter↔digit and camelCase → lowercase +
  diacritic fold (`unicode-normalization` NFD, MIT/Apache) → music
  recognition → noise removal. Tokens carry byte offsets and a position;
  extra tokens of one span share the position (Lucene-synonym style).
- **Canonical tokens**: BPM `120bpm` (+ `120`); key `key:<root>[m]` +
  `note:<root>`, roots in sharps (`Gb`→`f#`), major and unspecified share
  `key:<root>`; one-word keys also keep their folded spelling (`am`) so
  lowercase queries match. camelCase runs also emit the catenated run
  (`DnB` → `dn`+`dnb`, `b`), like ES `catenate_all`/`preserve_original`.
- **Ambiguity rules (conservative)**: `FM`/`AM`/`dB`/`EBM` and lowercase
  `am`/`fm`/`eb` are words; a bare letter is a key only right after a BPM
  (`Loop_120_C`); a bare number is a BPM only if 60–200, standalone, no
  leading zero, and the name has a loop word or a strong key, with exactly
  one candidate. Conflicting values → no prior. Folders (2 levels) give
  explicit values only. Priors use the same recognizer as the tokens.
- **Noise**: short stop list (formats, `bpm` alone, `bit/hz/khz`, `www/com`,
  `prod/free/copy/sample(s)`, `the/and/of/by`); padded numbers (`_01`),
  versions (`v2`), number+unit (`24bit`, `44.1kHz`, `48k`), sample rates.
  `loop`, `one shot`, vendor names stay. All configurable
  (`TokenizerConfig`).
- **Query mode** (`Tokenizer::for_query()`, `sonar tokenize --query`):
  users type lowercase, so an all-lowercase key spelling (`am`, `cm`,
  `eb`, `dbm`, `fm`) also emits `key:`/`note:` beside the plain word;
  bare letters (`a`) still don't. Index mode stays strict (`fm bass` is
  FM). T2.2 must OR the tokens that share a position when building a
  query (they are alternatives, not all required).
- **Cost**: ~0.7 µs per full path (release, M4 Pro), ~0.4 µs per prior.

## ADR-017 — Text index and query semantics (T2.2)
Accepted. `sonar_core::index` (Tantivy) + `sonar_core::search` (queries).
- **Engine**: `tantivy` 0.27 (MIT), `default-features = false`, features
  `mmap` + `lz4-compression` only. Text fields use the ADR-016 chain as a
  registered tokenizer, plus plural folding (`kicks` → + `kick` at the same
  position, Harman S-stemmer; `ANALYZER_VERSION` 2).
- **Schema** (`SCHEMA_VERSION` 1), one doc per present file: `file_id` (u64,
  indexed + fast), `name` (file name w/o extension), `path` (library root
  name + folders), `tags`, `category`, `kind` (effective = override, else
  detected). Positions on. Query weights name 3, tags/category 2, kind 1.5,
  path 1.
- **Location / freshness**: `<data dir>/index/text` + a version file
  (schema + analyzer). Version mismatch, unfinished build or unreadable
  index → wiped and rebuilt; a non-empty dir that is not ours is never
  deleted (`ForeignIndexDir`). `sonar search` builds a missing index on
  first use; `sonar scan` rebuilds an existing one; `sonar index` forces a
  rebuild. `refresh_files(ids)` exists for incremental callers (watcher,
  app).
- **Synonyms**: `docs/synonyms.tsv` (built in via `include_str!`), one
  group per line, members TAB/comma separated, `#` comments, multi-word
  members match as phrases, members go through the index chain. Lookup:
  `--synonyms <file>` > `<data dir>/synonyms.tsv` > built-in. Expanded at
  query time (editing needs no reindex). Alternatives at one position share
  one IDF (max doc freq of the group, like Lucene `SynonymQuery`); synonyms
  weigh 0.8 of the typed word.
- **Query**: whitespace chunks, `"..."` groups, `-chunk` excludes (with its
  synonyms). Every positive position is required (AND); each position is a
  dis-max (tie 0.1) over its alternatives × fields. Only exclusions / empty
  → all docs minus exclusions.
- **As-you-type**: without a trailing space the last positive chunk also
  matches as a prefix (constant score 0.5 × field weight, below exact
  hits). Prefixes are literal: no synonym expansion of partial words
  (`kic` finds Kick files; `kick` then adds BD) — expanding `bas` would
  pull in `bassdrum`. A tail that may still become a stop word (`bp`) only
  boosts. Exclusions never use prefixes.
- **CLI**: `sonar search [--json] [--limit N] [--no-prefix] [--synonyms F]
  <query...>`; options come **before** the query words (the query takes
  hyphen values so `-vinyl` works). `sonar index` rebuilds.

## ADR-018 — Filters, sort and facets (T2.3)
Accepted. `sonar_core::search::{Filters, Sort, Facets}` on extra index
fields; `SCHEMA_VERSION` 2 (old indexes rebuild).
- **Fields** (`ff_*`): folded name (sort), category, kind, tags (raw
  folded strings, indexed + fast; tags multi-valued), key code
  (`root*2+minor`, mode-less = major), BPM ×100, duration ms, library id,
  favourite/hidden flags (u64). Unknown → no value.
- **Effective value** = override (explicit NULL clears, no fallback) ??
  analysis column ?? filename prior (BPM/key only, ADR-016, computed at
  index time). Duration from `audio_meta`. No migration needed.
- **Semantics**: sets any-of, tags all-of, ranges inclusive (`None` end =
  open), unknown values never match an active filter. Strings compare
  folded. Filters add no score (const 0), so BM25 order is unchanged.
- **Compatible keys**: Camelot wheel — same key, ±1 (fifth up/down, same
  mode), relative major/minor; 4 keys each.
- **Hidden**: indexed with a flag, excluded unless `include_hidden`.
- **Sort**: relevance (default; without text → name), name, bpm, key
  (root then mode), duration, category, kind, library; asc/desc. Unknown
  values last in both directions; ties by file id.
- **Facets**: categories, kinds, keys, libraries, tags, favourites count;
  BPM/duration not faceted (sliders, not chips). Any-of dimensions and
  the favourites toggle ignore their own filter (multi-select chips),
  counted in the same pass as the hits (drill-sideways, Lucene
  `DrillSideways`); tags (all-of) count the final result. Without facets
  every filter is in the Tantivy query.
- **CLI**: `sonar search [--category C,..] [--kind K] [--bpm A-B]
  [--key K [--compatible]] [--duration A-B (s, or ms suffix)]
  [--library ID|PATH] [--tag T]... [--favorites] [--include-hidden]
  [--sort F[:desc]] [--facets] [query...]`.
- **Cost** (500k synthetic docs, in RAM, release, M-series): text `kick`
  p95 2.9 ms; browse all + facets 23 ms; BPM range + facets 1.4 ms.

## ADR-019 — Aspect vectors and library normalization (T3.2)
Accepted. `sonar_core::dsp::aspects`, `ASPECTS_VERSION` 1; table of dims in
ARCHITECTURE §5 and per-index docs in each submodule.
- **Dims**: Spectrum 38, Timbre 57, Pitch 18, Amplitude 22 (raw), Overall
  135 (normalized only). Fixed for any input length; silence has a
  documented value per dimension; every value finite.
- **c0 dropped** from Timbre (c1–c19 mean/std/delta-std): c0 is the frame's
  mean log-mel level = loudness, already in Amplitude. Delta *std* only —
  delta mean telescopes to ≈ 0. Spectrum/Timbre also read the log-mel
  clipped 80 dB below the file max (librosa `top_db`), so a gain change does
  not move them (a pure sine ±20 dB: Timbre/Pitch/Spectrum distance < 10 %
  of sine-vs-noise after normalization).
- **Active frames**: frame stats skip frames > 60 dB below the loudest
  (tails, padding).
- **Pitch**: reusable `dsp::yin` (de Cheveigné & Kawahara 2002, FFT
  difference function, threshold 0.15, parabolic interpolation on d;
  < 0.2 cent on sines 55 Hz–1.76 kHz) and `dsp::chroma` (Fujishima PCP,
  soft 2-class assignment, 100 Hz–5 kHz). f0 as `log2(f0/440)`; pitch class
  on the unit circle; unvoiced → 0 with voiced fraction 0. Chroma not
  key-rotated.
- **Amplitude**: 1.45 ms block energy smoothed over 16 ms (low notes don't
  dip at zero crossings); Peeters (2004) attack / decay / effective
  duration as log10 s; crest and loudness over blocks within 40 dB of peak.
- **Normalization**: `NormStats` per aspect (Welford, Chan merge for rayon
  fold/reduce, serde JSON); z = (x − mean)/max(std, 1e-3), clamped ±8;
  empty stats = identity. `LibraryNorm` bundles the four + version; push /
  merge validate dims and change nothing on error.
- **Overall** = concat of z-scored aspects × `wₐ/√dimₐ`, `wₐ` = 1 for all
  four (each aspect ≈ equal share of the squared distance); tune in T4.4.
- **Cost**: aspects 10.9 ms per 30 s file (analyzer + aspects 14.1 ms), 1
  thread, release, M-series. Wiring into jobs/vector store: T4.1/T4.2.

## ADR-020 — Loop vs one-shot classifier (T3.3)
Accepted. `sonar_core::classify` (`classify_kind`, `kind_scores`,
`kind_hint`), `KIND_VERSION` 1; `sonar classify <file>... [--json]
[--no-names]`.
- **Evidence** from the existing DSP `Features` (no second STFT):
  *periodicity* = best normalized autocorrelation of the onset envelope at
  0.25–1.5 s lags after a 1 s moving-average detrend (Ellis 2007 §3.1,
  Scheirer 1998), lag must fit twice in the window; *pulse* = std of the
  detrended envelope; *true duration* (not the 30 s window); *tail* =
  loudest frame of the last 15 % (≥ 0.25 s) re the loudest frame, unknown
  when the file is longer than the window.
- **Decision**: fixed, hand-set logistic. `z = 12·(P·min(1, pulse/1 dB) −
  0.2) + clamp(1.5·log2(dur/2 s), −3, +1) + clamp((tail+12 dB)/8, −1, 0)`;
  Loop iff `z > 0`, `p_loop = σ(z)`. Rhythm is the primary cue and must be
  strong in absolute terms (the pulse gate stops steady tones/noise from
  scoring on scale-free ripple); short files lean one-shot; long files
  lean loop but cannot outvote missing rhythm; a decayed end leans
  one-shot, a sustained end is no evidence (steady tones are sustained).
- **Filename hint** (lower-weight prior): ADR-016 tokenizer on the stem,
  then ≤ 2 parent folders; loop words/tempo tokens → Loop, `oneshot`/`hit`/
  `one shot` → OneShot, conflicts → none. Adds ±2 to `z`: less than clear
  rhythm (≥ +3.6) or a short decaying hit (≈ −4), so it only decides
  ambiguous audio (rhythm-less pad loops). `source` records whether the
  hint flipped the audio decision. Silent/empty audio → hint's kind, else
  `Fallback` one-shot with confidence 0. Eval and the AC test pass no hint.
- **Truth mapping** for fixtures/eval: `loop` → Loop; `one_shot`, `tone`,
  `noise` → OneShot; `silence`/corrupt excluded. Fixtures: 105/105 with
  names hidden.
- **Cost**: 0.6 ms per 30 s window on top of the analyzer (3.2 ms), 1
  thread, release, M-series. Not yet wired into jobs/store (pipeline task).

## ADR-021 — Loop tempo (BPM) and kind/tempo storage (T3.4)
Accepted. `sonar_core::tempo` (`estimate_tempo`, `loop_tempo`),
`TEMPO_VERSION` 1; `sonar bpm <file>... [--json] [--no-names]`.
- **Envelope**: accent = half-wave rectified flux of μ-law compressed
  (μ = 100) mel band powers (Klapuri, Eronen & Astola 2006), from the
  existing log-mel (no second STFT). The dB onset envelope (T3.1) makes
  ghost notes/hats nearly as strong as kick/snare and flattens the metrical
  hierarchy. Triangular smoothing (5 frames), 1 s detrend (Ellis 2007).
  When the window holds the whole file (a loop) the envelope, its
  autocorrelation and the beat grid wrap around (circular), and frame 0
  gets the flux from the last frame so the downbeat is kept.
- **Tempo family**: comb salience = mean normalized ACF at every multiple
  of the beat period up to 8 s / half the window (Scheirer 1998; Grosche &
  Müller 2011 autocorrelation tempogram), on a 0.1 BPM grid over 40–250
  BPM, parabolic peak refinement; best peak × weights wins.
- **Octave correction**: ACF alone ranks the half-bar above the beat for
  backbeat/breakbeat patterns, and no tempo-only prior can then get both an
  85 and a 175 BPM breakbeat right. The ×¼…×4 alternatives of the family
  winner are re-scored by **clock fit** (Povel & Essens 1985): best-phase
  beat grid, recall (accent mass on ticks) × precision (ticks filled,
  relative to the median strong event). Weights: log-Gaussian tempo prior
  (centre 110 BPM, σ 1.5 octaves: a tie-breaker); **loop-length prior**
  when the window is the whole file: `bpm·duration/60` within 0.2 beats of
  a whole count `n` (true decoded duration, not the 30 s window), `n` =
  4·2^k → 1, other whole 4/4 bars 0.85, 3/4 bars 0.7, other 0.4, off-grid
  0.3; filename BPM ×3 within 3 %.
- **Refinement**: a winner that fits the loop length is snapped to exactly
  `n·60/duration` (`beats` reported); otherwise the parabolic estimate.
- **Filename prior** (optional, eval hides it): weights candidates
  (octave ties), raises confidence when it agrees, and is the value
  (`source = filename`, confidence 0.6 explicit / 0.4 bare number) when
  detection is missing or below confidence 0.2.
- **Confidence** = min(1, salience/0.5) × score margin over the best other
  octave alternative.
- **Gating**: `loop_tempo` returns None unless the T3.3 classifier says
  Loop (not Fallback); `KindTempo::new` drops a tempo for non-loops too.
  The text index no longer fills a one-shot's BPM from its filename prior.
- **Storage / pipeline**: the `dsp` job stage (one `Analyzer` per worker
  thread, like the decoder) runs features → `classify_kind` (name hint) →
  `loop_tempo` (name BPM prior) and the coordinator upserts only
  `analysis.kind, kind_conf, bpm, bpm_conf, bpm_source` (other columns
  untouched; Fallback → kind NULL; one-shot → bpm NULL) in the batch
  transaction. Jobs `DSP_VERSION` 2 → 3 so every file is re-analysed. The
  index already used analysis BPM over the filename prior (ADR-018).
- **Results** (fixtures, names hidden, `tests/tempo.rs`): 33/34 loops
  within ±1 BPM (97.1 %), octave errors 1/34 — the half-time pattern at 140
  (→ 70), documented as ambiguous by the generator; no one-shot gets a BPM.
- **Cost** (release, M4 Pro, 1 thread): tempo 0.1–0.4 ms on short loops,
  0.6–1.0 ms per 30 s window (analyzer 3.4 ms). `bench analyze --stages
  dsp`: 8776 → 5804 files/s (14 threads, warm cache) with analyzer +
  classify + tempo wired in.

## ADR-022 — Key and one-shot note, key/pitch storage (T3.5)
Accepted. `sonar_core::key` (`estimate_key`, `key_and_note`,
`NoteTracker`), `KEY_VERSION` 1; `sonar key <file>... [--json]
[--no-names]`.
- **Tonal chroma** from the existing STFT (no second FFT): per frame,
  prominent sinusoidal peaks (≥ 10 dB over bins ±3–4) in 80 Hz–5 kHz,
  log-parabolic refinement (Smith & Serra 1987), magnitude into a 10-cent
  pitch-class histogram; global tuning = weighted circular mean of the
  deviation from 12-TET (Harte & Sandler 2005), folded to 12 classes
  (HPCP idea, Gómez 2006 ch. 3). Frames > 60 dB below the loudest skipped.
  *Tonal ratio* = peak power ÷ band power.
- **Key**: Krumhansl–Schmuckler correlation against the 24 Temperley
  (1999) profiles, each harmonically expanded (4 harmonics, decay 0.6;
  Gómez 2006 §4.2). Gate: tonal ratio ≥ 0.5, ≥ 3 pitch classes at ≥ 25 %
  of the strongest, best r ≥ 0.5. Confidence = r × min(1, margin over
  runner-up / 0.1). Drums, noise, clicks and silence get no key.
- **Note** (one-shots only): YIN (`dsp::yin`, de Cheveigné & Kawahara
  2002) 30 Hz–4.3 kHz, window 1024 / hop 512 on ≤ 3 s from the onset
  (−40 dB of peak); frames within 30 dB of the loudest; energy-weighted
  median MIDI, then mean of frames within ±0.5 st, rounded. Gate: stable
  frames ≥ 60 % of active energy (kick sweeps, noise → none). Signals
  shorter than one frame get a signal-sized tracker. A one-shot with a note
  takes the note's pitch class as a **mode-less key** (so the key filter
  finds bass/808 hits); loops never get a note.
- **Filename prior** (optional, eval hides it): +0.1 correlation for keys
  matching the name key (both modes if the name has none), lends its mode
  to a note on the same root, raises confidence (noisy-or) when it agrees,
  and is the value (`source = filename`, confidence 0.6 explicit / 0.4
  bare letter) when audio has no key or one below confidence 0.2.
- **Storage**: no migration (columns exist since 0001). `dsp` stage keeps
  one `NoteTracker` per worker and upserts only `analysis.key, key_conf,
  key_source, pitch_midi, pitch_conf` (`store::key_pitch`) in the batch
  transaction; missing values store NULL. Key stored in display form
  (`C#m`, `D#maj`, `A` mode-less), read back by `parse_key`; the index
  already prefers `analysis.key` over the name prior. Jobs `DSP_VERSION`
  3 → 4.
- **Results** (fixtures, names hidden, `tests/key.rs`): key 16/16 (100 %,
  tonic + mode), note 52/52 (100 %, exact MIDI); no drum/noise/silence
  fixture gets a key or note.
- **Cost** (release, M4 Pro, 1 thread, warm): key 1.7–1.8 ms per 30 s
  tonal window, note 1.3 ms (30 s window, ≤ 3 s analysed), 0.1–0.6 ms on
  short files. `bench analyze --stages dsp`: 5804 → 4545 files/s
  (14 threads, warm cache).
