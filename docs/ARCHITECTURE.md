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
`classify` (loop/one-shot, category), `embed` (CLAP via `ort`), `text`
(tokenizer, synonyms), `index` (Tantivy + vector store), `search` (query
model, hybrid ranking), `dupes`, `jobs` (pipeline, priorities, cancellation,
progress), `eval`.

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
  the meter (0 s windows); `Dsp` measures peak/loudness.
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
- Watching: FSEvents via `notify`; offline volumes keep cached rows,
  marked `online=0`.

## 5. Features for similarity (aspects)
Per file, fixed-length vectors, z-score normalized with library stats:
- **Spectrum**: mel-band energy distribution stats (mean/std), centroid,
  rolloff, flatness.
- **Timbre**: MFCC 1–20 mean/std + deltas.
- **Pitch**: f0 (YIN/pYIN) median, confidence, chroma summary.
- **Amplitude**: envelope sampled on fixed time grid (log-time), attack time,
  decay, crest factor, loudness.
- **Overall**: weighted concat (or CLAP audio embedding blended in).
Similarity = Σ wᵢ · dᵢ(q, x) over aspects (cosine/L2 on normalized vectors).

## 6. Search
Query = `{ text?, filters, sort?, similar_to?: {file_id | external_path,
weights} }`.
1. **Filters** → Tantivy fast fields / bitset of allowed docs.
2. **Text** → BM25 over tokenized name/path/tags + synonyms; exclusions.
3. **Semantic** → CLAP text embedding → kNN over CLAP audio vectors.
4. **Fusion** → reciprocal-rank fusion (start) → tuned weighted fusion
   (later, driven by eval).
5. **Similarity mode** → brute-force SIMD over filtered set if < ~200k,
   else ANN on Overall then aspect-weighted re-rank of top-K.
Latency budget p95 < 50 ms at 500k docs. Queries are cancellable; the UI
always shows the latest query only.

## 7. FFI surface (sketch)
`open(home)`, `add_library(path)`, `remove_library`, `start_indexing`,
`pause/resume`, `progress() -> Progress`, `search(Query) -> Page<Row>`,
`row(file_id)`, `waveform_peaks(file_id, n)`, `similar(...)`,
`set_override`, tags/collections/favorites CRUD, `duplicates()`.
Pages are small structs (ids + display fields); the table fetches rows lazily.

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
