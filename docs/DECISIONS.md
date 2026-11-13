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
Accepted (model: ADR-026). Audio+text joint embeddings for
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

## ADR-023 — Vector store: mmap aspect matrices outside SQLite (T4.1)
Accepted. `sonar_core::index::vectors`; `sonar vectors [--compact] [--json]`.
- **Layout**: `<db dir>/index/vectors/gen-<n>/` with `ids.i64` + one
  matrix file per stored aspect (spectrum, timbre, pitch, amplitude), each
  with a 64-byte header (format version, kind, dims, value size,
  `ASPECTS_VERSION`, committed rows). Raw `f32` rows, read via `memmap2`
  (MIT/Apache-2.0, already in the tree via tantivy). Overall is not stored;
  it is built from normalized aspects at query time.
- **Mapping**: row ↔ file id rebuilt in memory from `ids.i64` on open; a
  re-analysed file appends a new row, last row wins, older rows are dead.
- **Crash safety**: append writes rows, then all headers; open takes the
  minimum header count and a writer truncates any torn tail. Single writer
  (lock file), many readers. Corrupt file / version or dims mismatch →
  error for readers; a writer resets to an empty store and the dsp stage
  reconciles.
- **Compaction**: new `gen-<n+1>` with live rows only + exact recomputed
  norm, switched by atomic rename of `CURRENT`. Pipeline compacts at ≥ 25 %
  dead rows.
- **LibraryNorm**: `gen-<n>/norm.json`, merged per batch (Chan); rebuilt by
  rayon fold/merge when missing, corrupt or behind the rows.
- **Pipeline**: dsp stage computes raw aspects and appends them before the
  batch's SQLite commit; at stage start it reconciles with `file_stages`
  (orphan rows dropped, files analysed without vectors re-queued). Jobs
  `DSP_VERSION` 4 → 5.
- **Result**: 1M × 135 f32 (548 MB) opens in 17 ms (release, warm cache);
  first full pass 87 ms (`tests/vectors.rs`, ignored).

## ADR-024 — Similarity query: distance, weights, filters (T4.2)
Accepted. `sonar_core::similar`; `sonar similar <id | --file PATH>
[--weights a=w,..] [filter flags of sonar search] [--limit N] [--json]`.
- **Distance**: raw rows z-scored on the fly with the library stats (same
  floor/clamp as `LibraryNorm`). Per aspect dₐ = RMS z-difference
  (L2/√dimₐ; ≈ √2 for unrelated files), combined as the weighted mean
  D = Σ wₐ·dₐ / Σ wₐ (ARCHITECTURE §5). Measured on the fixture library
  (34 tonal + drum one-shot seeds / 75 seeds incl. chords, loops, noise;
  same-family top-1, top-3 majority): Σ of squared dₐ (= L2 on Overall)
  29/34, 23/34 (68/75); **Σ dₐ (chosen) 34/34, 29/34 (74/75, 68/75)**;
  per-aspect L1 34/34, 31/34 (74/75, 70/75); cosine 32/34, 30/34. L2
  chosen over L1 (difference = 2 seeds, noise): it is the documented
  form and the per-aspect metric an ANN index (T4.3) can serve; L1 stays
  a T4.4 candidate.
- **Weights**: `AspectWeights` (spectrum, timbre, pitch, amplitude; ≥ 0,
  finite, Σ > 0, relative). Default all 1 (= `OVERALL_WEIGHTS`). CLI
  `--weights`: listed aspects only, `overall=w` adds w to all four.
- **Seeds**: file id → stored row (`Error::NoVectors` if none); external
  file → decode + aspects through the dsp stage's own path
  (`jobs::work::window_aspects`, same 30 s window/analyzer/extractor),
  panic-guarded; an external copy of a library file is found at D ≈ 0.
- **Filters**: same `Filters` as text search; `search::filtered_file_ids`
  runs them on the text index (hidden excluded unless asked) and only
  those ids with a live row are scored, brute force, bounded top-K heap,
  ties by file id; the seed file is never a hit. Cancel flag checked
  every 4096 candidates. New errors: `NoVectors`, `InvalidQuery`.
  Candidates come from the text index, so files analysed after its last
  build are missing until the Engine keeps it in sync (T5.5).
- **Pitch-only** finds the same pitch class (note or octave) top-1 for
  19/22 tonal seeds: chroma (12 of 18 dims) dominates Pitch; octave
  weighting is a T4.4 question.

## ADR-025 — Similarity at scale: exact parallel brute force, no ANN (T4.3)
Accepted. `sonar bench similar [--docs N] [--queries N] [--threads N]`
(500k synthetic files: real store, on-disk text index and vector store
with clustered random rows; 4 classes × 60 end-to-end `similar` calls,
filter and scoring phases timed separately; M4 Pro, 14 cores, release).
- **Before** (sequential scan): p50 / p95 ms overall 63 / 89, custom
  weights 53 / 89, filtered 5 / 33, unfiltered 65 / 91 — over budget;
  scoring ≈ 60 ms, `filtered_file_ids` ≈ 7 ms of it.
- **Chosen: rayon scan** over 4096-id chunks of the allowed set, one
  bounded top-K per worker, merged under the total (distance, file id)
  order: results bit-identical to the sequential scan for any thread
  count; cancel flag checked per chunk. Distance code unchanged.
  **After**: overall 13.5 / 16.9, weighted 12.6 / 13.9, filtered 2.1 / 6.3,
  unfiltered 12.7 / 15.6 (scoring ≈ 6.4 ms, filter ≈ 7 ms);
  `--threads 4`: 23.8 / 25.7 worst class.
- **Tried, rejected**: 8-lane accumulator kernel with branch-free
  clamp — no gain (single thread 60 → 60 ms scoring; the per-row cost is
  not the arithmetic) and it would change float rounding; reverted.
- **USearch** (2.x, Apache-2.0; scratch crate outside the repo) HNSW on
  the z-scored Overall (L2² = Σ msdₐ), M=16, efC=128, top-M then exact
  aspect-weighted re-rank, 200 seeds, recall@10 vs exact: build 12.7 s
  (f32, 343 MB) / 6.5 s (f16, 208 MB) on 14 threads, on top of the
  273 MB raw rows. Query 0.3–2 ms. Recall@10: default weights 0.92–0.985
  (ef 64–256, M 100–500); mixed weights 0.51–0.76; pitch-only 0.12–0.22;
  filters would need predicate search. Fast but inexact for default
  weights and wrong for custom weights/filters (the product's sliders).
- **Decision**: exact brute force only; no ANN index files, no `usearch`
  dependency. **Revisit** when a library passes ~1.5M vector rows or
  `sonar bench similar --threads 4` p95 exceeds 35 ms; then ANN only for
  default-weight, unfiltered queries (ef ≥ 128, re-rank ≥ 200) and brute
  force for the rest. The filter phase (7 ms, linear in matches) is the
  next cost to cut.

## ADR-026 — CLAP model: LAION CLAP HTSAT-unfused, int8 audio / fp32 text (T5.1)
Accepted. Measured with a throwaway Rust crate outside git (`ort`
2.0.0-rc.10 = ONNX Runtime 1.22, own front end per the spec below).
- **Model**: `laion/clap-htsat-unfused` (Wu et al. 2023, arXiv:2211.06687;
  HTSAT-tiny audio + RoBERTa-base text, trained on LAION-Audio-630K).
  ONNX: `Xenova/clap-htsat-unfused` at commit
  `c28f2883575e590e04d3146ff0713c2448d691ba` (transformers.js export of
  those weights, separate audio and text graphs with projection heads).
- **License**: weights Apache-2.0
  (<https://huggingface.co/laion/clap-htsat-unfused>, card `license:
  apache-2.0`); LAION-AI/CLAP code CC0-1.0. The Xenova repo declares no
  license of its own; it is a format conversion + quantization of the
  Apache-2.0 weights (`base_model: laion/clap-htsat-unfused`), so
  Apache-2.0 governs. **Commercial redistribution inside the paid app is
  allowed**; duties: ship the Apache-2.0 text and an attribution notice
  ("LAION CLAP, converted to ONNX and int8-quantized"). Residual risk,
  shared by every public CLAP: LAION-Audio-630K mixes sources under other
  terms (some non-commercial); no dataset audio is shipped.
- **Files** (`bash scripts/fetch-models.sh` → `${SONAR_MODELS_DIR:-<repo>/models}/
  clap-htsat-unfused/`; pinned URLs + SHA-256 in the script, re-run is a
  no-op, bad file deleted and fetched again, failed check exits 1):
  `audio_model_quantized.onnx` 34.3 MB, `text_model.onnx` 501.5 MB,
  `tokenizer.json` 2.1 MB, `preprocessor_config.json`, `config.json`
  (reference). Total ≈ 538 MB.
- **Graphs**: audio `input_features` f32 `[B,1,1001,64]` → `audio_embeds`
  f32 `[B,512]`; text `input_ids` i64 `[1,L]` → `text_embeds` f32 `[1,512]`.
  The text graph has **no attention_mask input**: one query per run, never
  pad. Outputs are **not** L2-normalised (norms 3.8–6.8 audio, 4.0–4.6
  text): normalise, then cosine = dot.
- **Audio front end** (= HF `ClapFeatureExtractor`, non-fusion path;
  fusion off):
  - mono (channel mean) at **48 kHz**; clip = 480,000 samples (10 s).
  - longer: first 480,000 samples (HF crops at a random offset; we stay
    deterministic). Shorter ("repeatpad"): tile the clip
    ⌊480000/len⌋ times, then zero-pad to 480,000. Empty audio (len 0)
    is rejected (no embedding), never divided by.
  - STFT: n_fft 1024, periodic Hann 1024, hop 480, centred with **reflect**
    padding of 512 per side → 1001 frames; power |X|².
  - mel: 64 bands, 50–14,000 Hz, **Slaney** mel scale + Slaney area norm
    (librosa `htk=False, norm="slaney"`), on 513 bins at k·46.875 Hz.
  - 10·log10(max(mel, 1e-10)), ref 1.0, no top_db clip, no further
    normalisation (the model has its own BatchNorm). Layout time-major
    `[1001][64]`. Cost: 1.8 ms/clip on one core (realfft).
  - Rate: the 22.05 kHz analysis decode upsampled to 48 kHz loses
    11–14 kHz: embedding cosine vs native→48 kHz mean 0.968, min 0.81
    (hats, noise); fixture zero-shot unchanged. Resample from the native
    rate to 48 kHz for embed.
- **Tokenizer**: RoBERTa byte-level BPE from `tokenizer.json` (HF
  `tokenizers` format, case-sensitive), `<s>`=0 … `</s>`=2 added;
  truncate to 77 tokens **including** `<s>`/`</s>` (content ≤ 75, then
  add the specials; LAION training length). "a kick drum" →
  `[0, 102, 3151, 12638, 2]`.
- **int8 vs fp32** — zero-shot top-1, prompts "a kick drum", "a snare
  drum", "a closed hi-hat", …: fixtures 95 files / 10 classes (coarse:
  64 files / 6 classes kick, snare, hi-hat, sine, white noise, drum loop);
  silver: 420 local one-shots, 7 classes by folder name (kick, snare,
  clap, hi-hat, cymbal, bass, synth), 60 each; NN-1 = nearest audio
  neighbour has the same class.

  | audio / text | fixt. 10 | fixt. 6 | silver | silver NN-1 |
  |---|---|---|---|---|
  | fp32 / fp32 | 0.800 | 0.984 | 0.638 | 0.871 |
  | **int8 / fp32** | 0.800 | 0.984 | 0.631 | 0.874 |
  | int8 / int8 | 0.789 | 0.984 | 0.619 | 0.874 |

  Audio fp32 vs int8 cosine mean 0.9984, min 0.9949 → **audio int8**
  (1.5–2× faster, 34 vs 118 MB). Text int8 cosine mean 0.979, min 0.942
  and −1.2 to −1.7 pt silver → **text fp32** (fp16 text: same silver
  accuracy, 251 MB, but 15.9 ms/query on CPU).
- **Speed** (Apple M4 Pro 10P+4E, 48 GB; release; CPU EP, graph opt
  level 3). Audio int8: one session × 10 intra threads 61 (batch 1) /
  76 (16) / 72 (32) clips/s; **N sessions × 1 thread: 8 → 122, 10 → 158
  (batch 1), 14 → 153 clips/s**. Audio fp32: 42 / 50 / 47; 5 sessions ×
  2 threads 66. CoreML EP: dynamic shapes 26–39 clips/s; with free-dim
  overrides + MLProgram fp32 batch 16 79 clips/s but 33 s compile per
  session load, int8 51; NeuralNetwork format 12–14. Text fp32 4.8 ms /
  query (4 threads; int8 2.0, tokenize 4 µs), session load 0.2 s.
  DSP is ≈ 3 ms CPU per file (≈ 4.5k files/s), so CLAP dominates:
  **> 100 files/s is reachable on CPU** with ≥ 8 single-thread int8
  sessions; CoreML (GPU/ANE) is a possible add-on next to them, not the
  default. Memory per session not measured.
- **Rejected**: `laion/larger_clap_general` (Apache-2.0, HTSAT-base,
  music+speech data): same quality here (silver 0.636, NN-1 0.883;
  fixtures coarse 0.938), ~2× slower (int8 70 clips/s with 10 sessions),
  78 MB int8 audio — revisit if golden NL search needs it.
  `laion/clap-htsat-fused`: 4-channel fusion input with random chunks
  (non-deterministic), slower, few samples exceed 10 s. `microsoft/msclap`:
  MS-PL (not on the ADR-004 list), no ONNX. Own export (optimum/torch):
  needs a Python toolchain; the pinned, checksummed hosted export is the
  same weights.

## ADR-027 — Embed module: ort session pool, CLAP store, Embed stage (T5.2)
Accepted. Implements ADR-026 in `sonar_core::embed` (cargo feature `embed`,
on for the CLI; the FFI build gets a stub until T5.6).
- **Runtime**: `ort` =2.0.0-rc.10 (MIT OR Apache-2.0, ONNX Runtime 1.22) with
  default `download-binaries`: `ort-sys`' build script downloads the
  prebuilt onnxruntime at build time (needs network on first build; its
  build-only deps `ureq`/`native-tls`/`tar`/`webpki-root-certs` are not
  linked into Sonar). The runtime itself is MIT. `webpki-root-certs` is
  CDLA-Permissive-2.0 (a permissive data license): scoped exception in
  `core/deny.toml`. `cargo deny check advisories` flags `paste` (via
  `tokenizers`) and `basic-toml` (via uniffi) as unmaintained; advisories are
  not part of check.sh, left open (ROADMAP Later).
- **Audio**: a pool of N single-thread int8 sessions (default one per
  performance core), each on its own thread, fed through a bounded channel;
  inputs `[B,1,1001,64]` from the ADR-026 front end (`embed::front`,
  decode at the native rate, resample to 48 kHz); outputs L2-normalised.
  CPU EP by default; CoreML selectable (`--ep coreml`, `SONAR_EMBED_EP`),
  falls back to CPU if it cannot load. **CoreML is not the default**:
  ADR-026 measured it at 26-79 clips/s vs 150+ on CPU, with 33 s compile
  per session load.
- **Text**: one fp32 session, RoBERTa tokenizer, 77 tokens, no padding.
- **Store**: `index/clap/{ids.i64,emb.f32,meta.json,LOCK}` beside the aspect
  vectors; 512 x f32 rows, append-only, rows written before ids so a torn
  append is cut on open; re-embedding supersedes (new row wins); mmap
  readers, single locked writer. SQLite stage rows are the source of truth
  for "done"; the store is appended before the batch commit.
- **Stage**: `Embed` is the last and lowest-priority stage; it is skipped
  cleanly (not failed) when the model directory is missing or the build has
  no `embed` feature. CLI: `sonar embed`, `sonar embed-text`,
  `sonar analyze --stages ... embed`, `sonar bench embed`.
- **Throughput** (Apple M4 Pro 10P+4E, release, CPU EP, 10 sessions,
  batch 1): model only 157 clips/s; decode + front end 2.1 ms/file on one
  thread; **end to end 154 files/s** over a 1000-file generated library
  (budget >= 100 files/s: met). Model load 4 s. Scaling is per
  performance core (~15.5 clips/s each): a base M4 (4P) defaults to 4
  sessions, ≈ 60 files/s estimated (not measured) — the documented best
  there; the M4 Pro figure is the measured one.

## ADR-028 — Category classifier: CLAP zero-shot + DSP naive Bayes + name hint (T5.3)
Accepted. `sonar_core::classify::category`; taxonomy of 17 sound-source
categories in `docs/categories.md` (mirrored by a unit test; loop/one-shot
stays a separate field).
- **Evidence, late fusion**: CLAP zero-shot (Wu et al. 2023) = best-prompt
  cosine per category (several sub-type prompts per class; max beat the
  CLIP prompt-embedding average by 2-3 points on silver); DSP = Gaussian
  naive Bayes (Duda/Hart/Stork §2.6) over 11 named aspect-vector cues
  (spectral always, envelope only for one-shots, f0 gated by tonality) plus
  a loop/one-shot prior. Profiles fitted on 22 of the 47 silver genre
  folders (hand rows for classes silver lacks: lead, pad, pluck, vocal,
  texture). `z = 10·clap + 0.2·dsp` (or `dsp` alone), `p = softmax(z/T)`.
- **Calibration**: temperature scaling (Guo et al. 2017) fitted on silver:
  `T = 0.96` fused, `2.69` DSP-only. `sonar eval` refits T on a hash-split
  half and reports held-out ECE.
- **Name hint**: a category word in the stem or the two nearest folders adds
  +3 to that calibrated logit (odds ≈ 20:1): decides ambiguous audio, cannot
  outvote clear audio. Source `filename` iff it changed the decision. The
  eval hides names, so measured numbers are audio-only.
- **Pipeline**: the `dsp` stage stores the DSP + name category
  (`DSP_VERSION` 6); the `embed` stage re-decides with CLAP from the stored
  aspect vectors + kind + duration and overwrites it (`EMBED_VERSION` 2). A
  DSP-only rerun never replaces a CLAP-refined row. CLI: `sonar classify`
  (`--no-clap`, `--no-names`), `sonar eval` (`--no-clap`).
- **Eval**: tracked fixture metric is the **DSP-only** top-1 / macro-F1 (no
  dependency on `models/`); the shipped (CLAP + DSP) and CLAP-only numbers
  are report-only. Measured (audio only): fixtures DSP 33.9 % / CLAP + DSP
  53.6 % (synthetic, 56 labelled); silver (7818 files, folder labels) DSP
  65.8 % / CLAP-only 71.3 % / CLAP + DSP 76.0 %, ECE 1.8 %; the two
  alphabetical halves of the genre folders agree within 2.4 points. The
  silver numbers are **partly in-sample** (profiles, fusion weights and T
  were fitted on silver). Shipped accuracy *with* the name hint is not
  measured (silver labels come from the same words); golden is the real
  check.
- **Consequences**: the golden ≥ 90 % target is open until the founder's
  golden set exists; silver is one vendor library with noisy folder labels
  (perc folders hold snares/claps). Re-fit weights/profiles when golden
  arrives (bump `CATEGORY_VERSION` + both stage versions).

## ADR-029 — Hybrid search: BM25 + CLAP kNN, reciprocal-rank fusion (T5.4)
Accepted. `sonar_core::search::{hybrid, semantic}`; CLI `sonar search
--mode hybrid|text|semantic`; `sonar bench search --hybrid`.
- **Legs**: text = the existing `search` (ADR-017/018), top 200 (or the
  limit if larger). Semantic = the query's positive words (exclusions and
  quotes dropped) through the CLAP text tower, then **exact** cosine kNN
  (dot product of unit vectors, 16-lane accumulators) over the allowed set
  = filters minus the `-word` exclusions (same synonyms as text), rayon
  scan in 4096-id chunks with mergeable top-K, as ADR-025. The text leg
  runs in parallel with the query embedding and the allowed-set pass.
- **Fusion**: Reciprocal Rank Fusion (Cormack, Clarke & Büttcher 2009),
  `Σ 1/(60 + rank)`; rank-only, so BM25 and cosine never need calibrating.
  Ties: better best rank, then file id. Weighted fusion only when golden
  eval shows a gain (ARCHITECTURE §6).
- **Current rows only**: `SemanticIndex::open` keeps rows of files with an
  `embed` stage result `ok`/`partial` (`embedded_file_ids`); a changed file
  whose re-embed failed or is pending is never returned (closes the T5.2
  review item for search).
- **Fallback, never an error**: no positive words (browse), a field sort,
  no model / `embed` feature, no embeddings, or a text-tower error → text
  mode, with `fallback` saying why. Facets: hybrid returns the text leg's
  counts; semantic none.
- **Ownership**: `SemanticSearch` (text encoder behind a mutex + mmap'd
  index) is loaded once and shared by `&self` queries; the Engine (T5.5)
  holds it next to the text index. `QueryEmbedder` lets tests/bench inject
  a stand-in encoder.
- **Eval**: NDCG@10 / recall@50 on 19 fixture queries judged from truth
  (EVAL.md "Text / NL search"); BM25 keys tracked, CLAP/hybrid report only
  (model may be absent). Golden `text_queries.jsonl` supported.
- **Numbers** (M4 Pro, release, 500k synthetic files, every query with
  facets, real CLAP text tower; 1 GB of clustered random 512-d rows):
  text encode p50 / p95 3.9 / 5.2 ms; p50 / p95 ms hybrid NL queries
  13.4 / 13.9, hybrid every keystroke 13.3 / 34.5, hybrid + filters
  5.9 / 10.7, semantic only 13.3 / 14.0, all under the 50 ms budget. Text
  classes unchanged (as-you-type 29.5, text+filters 4.3, browse 27.3 /
  29.3, ranges 9.6 p95; T2.4: 29.3 / 4.3 / 27.7 / 29.4 / 9.7). Fixtures
  (19 queries): NDCG@10 / recall@50 BM25 29.0 / 26.8, CLAP 89.2 / 98.9,
  hybrid 91.3 / 100. BM25 is weak there by construction (names hidden;
  only detected category/kind words to match).
- **Later**: per-keystroke hybrid re-embeds the query (≈ 5 ms each); the
  app may debounce the semantic leg. Effective BPM/key are not text tokens,
  so "120 bpm" in a query only matches names.

## ADR-030 — Engine facade (T5.5a)
`sonar_core::engine::Engine` is the one long-lived object behind the FFI and
the CLI (`scan`, `watch`, `analyze`, `search`); nothing outside it wires
store, index, watcher and jobs by hand any more.
- **Ownership**: per data directory it owns one `Store` connection (quick
  queries), the `TextIndex`, `Synonyms`, an optional `WatchHandle`, at most
  one `JobHandle`, a lazily loaded CLAP query side (`SemanticSearch`;
  text-only with the reason as `fallback` when model/embeddings are
  missing, reloaded after an embed batch or job end) and one `sonar-engine`
  thread. Scans, watcher and jobs open their own connections.
- **Threading**: `Engine: Send + Sync`, all methods `&self`; shared state
  behind short mutexes never held across a blocking call; callbacks given
  to the watcher/jobs capture only the bounded channel (256) to the engine
  thread, never `Inner`. Drop: stop watcher, cancel job, join thread.
- **Index sync, no rebuilds**: full rebuild only for a missing / unreadable /
  other-version index at open (`EngineOptions::rebuild_index`; CLI commands
  that merely maintain an index pass `false` and leave a missing one to the
  first search). Otherwise `TextIndex::refresh_files` with only touched ids:
  `ScanReport::touched` (new) for `Engine::scan_library` (before it
  returns), `WatchEvent::Scanned::touched` (new, not serialized) and the new
  `JobEvent::Committed { stage, file_ids }` (one per committed batch) via
  the channel; the engine thread coalesces what is queued into one refresh.
  So a `Scanned` event implies a fresh index and `wait_indexing` returning
  implies every committed batch is searchable (the end notice is ordered
  after the commits; a job that failed without one is found by a 250 ms
  poll that reads "finished" before draining the channel).
- **Search**: `Engine::search(SearchRequest)` (mode, filters, sort,
  `offset`/`limit`, prefix, facets) over `search::hybrid`. Latest query
  wins: each call swaps in a fresh cancel flag and sets the previous one;
  `hybrid_search` checks it between legs and per kNN chunk, and a search
  that was cancelled by the time it finishes returns `Error::Cancelled`,
  never stale hits. The flag is engine-wide (one search box per engine);
  `cancel_search` cancels the newest. Tantivy text queries are not
  interruptible (milliseconds), only discarded.
- **Watch -> analysis**: `start_indexing` *enables* indexing and starts a
  job; while enabled and idle, a scan (watcher or manual) that adds,
  changes or restores files starts a job; if one is running a rerun follows
  when it ends (the stage cursor is forward-only, so late files would
  otherwise be missed). `cancel_indexing` disables. Without
  `start_indexing` the watcher only keeps rows and index fresh (`sonar
  watch`). `remove_library` (new `Store::remove_library`, cascade) is a
  user action; vectors of removed files are reconciled by the next job.
- **Later**: none (`similar` and the user-data setters arrived in T5.5b, ADR-031).

## ADR-031 — User-data API (T5.5b)
Favourites, hidden, tags, collections and overrides are written only through
`Store` methods (`store/user_data.rs`, `collections.rs`, `overrides.rs`) and
the matching `Engine` methods; scan and analysis never touch these tables.
- **Multi-id, all-or-nothing**: setters take `&[i64]`, run in one
  transaction and fail with `Error::FileNotFound` (or `CollectionNotFound`)
  before changing anything if an id is unknown. They return how many files
  actually changed (re-setting is a no-op, 0).
- **Refresh**: the engine calls `refresh_files` for the given ids before
  returning, only if something changed and the index is built, so the next
  search sees it (large id sets use the engine's rebuild threshold).
  Collections are not in the index, so they never refresh. A setter plus
  refresh is ~87 ms on the 500k index (Later in ROADMAP).
- **Validation** (`Error::InvalidInput`, nothing written): BPM is a plain
  decimal number 20..=400 (no unit, sign or exponent; `128 bpm` is an
  error), rounded to 0.01, stored REAL; key is anything `parse_key` reads,
  stored canonically; category is a taxonomy id (or a spelling
  `Category::parse` knows); kind is `loop` / `one_shot` (`one-shot`
  accepted). Clearing an override deletes the row, so detection shows again.
- **Tags**: unique ignoring case, the first spelling is kept; names are
  trimmed, 1..=64 chars, no control characters; a tag nobody carries is
  deleted. **Collections**: names unique ignoring case, files appended in
  the order given, re-adding keeps the place.
- **CLI**: `sonar fav|hide|tag|collection|override` take ids or paths of
  library files; `sonar similar` now goes through `Engine::similar`.
