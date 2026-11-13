# Sonar — Architecture

## 1. Layers

```
┌──────────────────────────────────────────────────────────────┐
│ macOS app (Swift)                                            │
│  SwiftUI shell (windows, sidebar, inspector, settings)        │
│  AppKit where performance matters: NSTableView results,       │
│  waveform (CALayer/Metal), drag & drop (NSPasteboard)         │
│  Platform services: AVAudioEngine preview, FSEvents bridge,   │
│  bookmarks for external volumes                               │
├──────────────────────────────────────────────────────────────┤
│ Bridge: UniFFI (sonar-ffi) → SonarCore.xcframework            │
├──────────────────────────────────────────────────────────────┤
│ Rust core (platform-neutral, reused for Windows later)        │
│  scan · decode · analyze (DSP) · embed (CLAP/ONNX) ·          │
│  store (SQLite) · index (Tantivy + vectors) · search/rank ·   │
│  eval                                                         │
└──────────────────────────────────────────────────────────────┘
v2 only: plugin-host sidecar (C++/JUCE, separate process)
```

Rules:
- **All product logic lives in Rust.** Swift only renders and plays audio.
  If something could ever be needed on Windows, it belongs in Rust.
- The core is fully usable headless through `sonar-cli`. Every feature must be
  testable from the CLI before it gets UI.
- The core never blocks the caller: long work runs on core-owned worker
  threads and reports progress via callbacks/event polling.

## 2. Rust workspace (`core/`)

| Crate | Role |
|---|---|
| `sonar-core` | Library: domain model, store, scan, decode, analysis, embed, index, search, eval. Modules below. |
| `sonar-cli` | `sonar` binary: `scan`, `analyze`, `search`, `similar`, `stats`, `eval`, `bench`, `fixtures`. |
| `sonar-ffi` | UniFFI facade over `sonar-core` (created in M0). Thin; no logic. Static lib packaged by `scripts/build-core.sh` into `app/Generated/SonarCore.xcframework` + `SonarCore.swift`. |
| `uniffi-bindgen` | Build tool only: UniFFI bindings generator pinned to the workspace UniFFI version. |

`sonar-core` modules (create as needed, keep files < ~400 lines):
`model`, `store` (SQLite + migrations), `scan` (walk, identity, watch),
`decode` (symphonia + resample to analysis rate), `dsp` (stft, mel, mfcc,
pitch, onset, tempo, key, envelope), `features` (aspect vectors),
`classify` (loop/one-shot, category), `embed` (CLAP via `ort`; model, front
end and `scripts/fetch-models.sh` → `models/`: ADR-026), `text`
(tokenizer, synonyms), `index` (Tantivy + vector store), `search` (query
model, hybrid ranking), `dupes`, `jobs` (pipeline, priorities, cancellation,
progress), `engine` (long-lived facade over all of the above for the FFI
and CLI: libraries, indexing, watcher, incremental index sync, cancellable
search; ADR-030), `eval`.

Decode gapless: AAC/M4A priming and padding come from the edit list
(`elst`, ISO/IEC 14496-12 / ffmpeg semantics: `segment_duration` honoured,
unlike Apple's reader) or `iTunSMPB`; symphonia's `isomp4` ignores both, so
`decode::gapless` reads them (MP3 uses the LAME tag via symphonia).

### Preferred dependencies (permissive licenses only)
`symphonia` (decode), `rubato` (resample), `rustfft`/`realfft`, `rayon`,
`rusqlite` (bundled), `tantivy`, `usearch` (or SIMD brute force),
`ort` (ONNX Runtime; CoreML EP on macOS), `notify`, `twox-hash` (XXH3; not `xxhash-rust`, which is BSL-1.0),
`serde`/`serde_json`, `clap` (CLI), `thiserror`/`anyhow`, `tracing`,
`uniffi`, `criterion` (bench). **No GPL/AGPL/LGPL-static** (no aubio,
Essentia, libKeyFinder). Enforced by `cargo deny` (`core/deny.toml`).

## 3. Data model (SQLite, WAL mode)
Location: `~/Library/Application Support/Sonar/` (override with
`SONAR_HOME` for tests/CLI).

- `libraries(id, root_path, volume_uuid, bookmark BLOB, online, added_at)`
- `files(id, library_id, rel_path, size, mtime, quick_hash, content_hash,
   status, analysis_version, error)` — `mtime` in ns since epoch,
   `quick_hash` u64 stored bit-cast as INTEGER, `status ∈ {new, meta,
   analyzed, error, missing}`; schema in `store/migrations/*.sql`
   (versioned by `PRAGMA user_version`, tables STRICT)
- `file_stages(file_id, stage, version, outcome, message, done_at)` — PK
  `(file_id, stage)`, `stage ∈ {meta, dsp, embed}`, `outcome ∈ {ok, partial,
  error}`. A file is pending for a stage when it has no row or an older
  `version` than the stage's current one (ADR-013). Deleted by the scanner
  when content changes.
- `audio_meta(file_id, duration_ms, sample_rate, channels, bit_depth,
   peak_db, loudness_lufs)`
- `analysis(file_id, kind, kind_conf, bpm, bpm_conf, bpm_source, key,
   key_conf, key_source, pitch_midi, pitch_conf, category, category_conf,
   category_source)` — `*_source ∈ {filename, detected}`
- User data (never touched by re-analysis):
  `overrides(file_id, field, value)`, `tags`, `file_tags`, `collections`,
  `collection_items`, `favorites`, `hidden`.
- Effective value = override ?? detected ?? filename prior.
- Vectors are **not** in SQLite: per-aspect `f32`/`i8` matrices in
  memory-mapped files keyed by a dense row id, plus optional ANN index files.
- `analysis_version` lets us re-run only stale stages after algorithm changes.

## 4. Indexing pipeline
```
discover → identify (size+mtime+quick_hash) → decode (mono, 22.05 kHz,
cap 30 s for analysis, full length for duration) → basic meta →
DSP features + aspect vectors → classify → CLAP embedding (batched) →
write store + index
```
- Basic meta (`sonar_core::meta`, ADR-012) runs inside the same decode pass
  (`Decoder::decode_with_meta`): format + counted duration, sample peak and
  BS.1770-4 integrated loudness at native rate over all channels, first
  600 s; `sonar meta <file>...`. The pipeline's fast `Meta` stage skips
  the meter (0 s windows); `Dsp` measures peak/loudness. For intact
  uncompressed PCM WAV/AIFF the `Meta` probe reads the length from the
  header without touching the payload (ADR-015); everything else is counted.
  PCM WAV/AIFF is counted by the bytes each packet holds, so a truncated
  file reports exactly its surviving frames (`partial`); AIFF `COMM`
  `numSampleFrames` is the declared length (ADR-015 addendum T1.7d).
- Stages are idempotent and resumable; crash mid-scan = continue later.
  `sonar_core::jobs` (ADR-013) runs them: `Meta` (probe with 0 s windows:
  format + duration) for the whole store, then `Dsp` (30 s analysis decode +
  peak/loudness), then `Embed` (not implemented yet). Workers decode on a
  rayon pool; one coordinator thread commits each batch in one transaction,
  so `kill -9` leaves whole batches and a restart resumes from what is
  pending. Pause/resume/cancel, polled progress and events; `sonar analyze`.
- Priorities: metadata first for everything (fast, browsable), then DSP,
  then CLAP last (most expensive). UI shows partial results immediately.
- `rayon` pool for CPU stages; CLAP in batches on its own thread.
- Corrupt files: record `error`, never panic across FFI (catch_unwind).
- Watching (`watch`, ADR-014): FSEvents via `notify` as a *trigger* only; a
  debounced (400 ms quiet, 5 s cap) incremental rescan of the affected
  library does the work, serialized on one watcher thread. Offline volumes
  keep cached rows, marked `online=0`; a 2 s root poll detects unmount and
  remount and re-establishes the watch. `sonar watch` exposes it.

## 5. Features for similarity (aspects)
Frame features (`sonar_core::dsp`, `DSP_VERSION` 1; `sonar dsp <file>...`):
one `dsp::Analyzer` per worker computes the STFT power spectrogram once
(n_fft 2048, hop 512 ≈ 43 frames/s, periodic Hann, centered, zero-padded)
and derives log-mel (64 Slaney-scale/Slaney-normalized bands 20 Hz–Nyquist,
dB with −100 dB floor), MFCC c0–c19 (orthonormal DCT-II), HTK deltas and the
onset strength envelope (rectified log-mel flux, band mean, 80 dB range).
Later features take `&Spectrogram` / `FeatureMatrix` from `dsp::Features`.

Per file, fixed-length raw `f32` vectors (`sonar_core::dsp::aspects`,
`ASPECTS_VERSION` 1, ADR-019; `sonar aspects <file>... [--json]`). One
`AspectExtractor` per worker (owns the YIN plans, chroma map, DCT) next to
the `Analyzer`. Frame stats use *active* STFT frames only (within 60 dB of
the loudest frame); Spectrum/Timbre read the log-mel clipped 80 dB below the
file maximum (gain-invariant shape). Every value is finite; silence has a
documented value per dimension; same samples → bitwise-identical vectors.

| Aspect | Dims | Content |
|---|---|---|
| Spectrum | 38 | 16 log-mel band groups: mean dB minus their average (shape) + std; centroid (`log2(f/1 kHz)`), 85 % roll-off, flatness dB — mean/std each |
| Timbre | 57 | MFCC c1–c19 mean, std, delta std (c0 dropped: loudness lives in Amplitude) |
| Pitch | 18 | YIN (`dsp::yin`, 30 Hz–2 kHz): voiced fraction, mean confidence, median f0 `log2(f0/440)`, f0 IQR (oct), pitch class of the median as cos/sin; 12-bin chroma mean (C..B, not key-rotated; `dsp::chroma`) |
| Amplitude | 22 | 16-bin envelope (dB rel. peak → [0,1]) on a log-time grid 0, 5 ms … 20 s after sound start; log attack, log decay, log effective duration, crest factor dB, gated RMS dBFS, level fluctuation dB |
| Overall | 135 | concat of the z-scored aspects, each × `wₐ/√dimₐ` (`OVERALL_WEIGHTS` = 1, 1, 1, 1) — built after normalization |

Library normalization: `LibraryNorm` holds a `NormStats` (Welford mean/M2,
mergeable across rayon workers by Chan's formula, serde JSON) per aspect;
z = (x − mean) / max(std, 1e-3), clamped to ±8. Persisting stats and vectors
is the vector store's job (T4.1).
Similarity = Σ wᵢ · dᵢ(q, x) / Σ wᵢ over aspects, dᵢ = RMS difference of
the z-scored vectors (L2/√dimᵢ); `sonar_core::similar`, ADR-024.

## 6. Search
Query = `{ text?, filters, sort?, similar_to?: {file_id | external_path,
weights} }`.
1. **Filters** → Tantivy fast fields / bitset of allowed docs (ADR-018:
   effective category/kind/BPM/key/duration/library/tags/favourite in
   `ff_*` fields; compatible keys via Camelot; hidden excluded; facet
   counts drill-sideways in one collector pass; unknown values sort last).
2. **Text** → BM25 over tokenized name/path/tags + synonyms; exclusions.
3. **Semantic** → CLAP text embedding of the positive words → exact cosine
   kNN over the current CLAP audio vectors of the allowed set (filters
   minus exclusions), parallel brute force (ADR-029).
4. **Fusion** → reciprocal-rank fusion, k = 60, top 200 per leg
   (`search::hybrid`, ADR-029; modes hybrid / text / semantic, text-only
   fallback without the model) → tuned weighted fusion (later, driven by
   eval).
5. **Similarity mode** → exact parallel brute force over the filtered set
   (ADR-025: p95 ≈ 17 ms at 500k); ANN on Overall + aspect-weighted
   re-rank only for default-weight, unfiltered queries if libraries pass
   ~1.5M files.
Latency budget p95 < 50 ms at 500k docs. Queries are cancellable; the UI
always shows the latest query only: `Engine::search` is latest-query-wins
(a newer search flags the older one, which returns `Cancelled`, never stale
hits; ADR-030).

## 7. FFI surface (v1, ADR-032)
One UniFFI object, `SonarEngine` (`sonar-ffi`), over `sonar_core::engine::Engine`:
libraries, indexing control + polled progress/events, paged `search` (display
rows), `similar`, `similar_to` (weights, filters, external seed; ADR-035), `row(s)`, user-data setters, `waveform_peaks`, `preview_pcm`.
Every fallible export `throws FfiError`; the exact list, blocking rules,
cancellation and PCM format are in ADR-032. `duplicates(query)` /
`cancelDuplicates()` (exact + near groups, own cancel flag) are in ADR-036.

## 8. macOS app (`app/`)
- Project generated with **XcodeGen** (`app/project.yml`) — never hand-edit
  `.xcodeproj`. Target macOS 14+, Apple Silicon first (universal build later).
- Not sandboxed (direct distribution, hardened runtime, notarized).
- Structure: `App/` (entry, windows), `Features/` (Library, Search, Results,
  Preview, Similar, Organize, Duplicates, Settings), `Services/`
  (CoreBridge, AudioPreview, Bookmarks), `UI/` (shared components).
- Results table: `NSTableView` wrapped in `NSViewRepresentable`, view-based,
  row reuse, lazy row fetch from core.
- Preview: AVAudioEngine, pre-decoded buffer cache for the selected row ±2.

## 9. Testing strategy
- Rust unit tests per module; property tests for tokenizer/parsers.
- **Synthetic fixtures** generated by `sonar fixtures` (sines, chords,
  click tracks at known BPM, noise bursts, decays) → ground truth without
  copyrighted audio in the repo.
- **Golden set** (user's real samples, local only, gitignored) → `sonar eval`.
- Benchmarks (`sonar bench`, criterion) for scan, search latency.
- Swift: XCTest for bridge + view models; UI smoke via `xcodebuild test`.
