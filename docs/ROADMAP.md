# Roadmap

The autonomous loop works top to bottom. Rules:
- Pick the **first** unchecked task whose dependencies are done.
- `[HUMAN]` tasks are never done by agents and never block the loop: skip
  them and keep them listed under "Needs human" in `docs/STATUS.md`. The
  founder does not approve tasks; he tests at the end (M8).
- A task is checked `[x]` only when its acceptance criteria (AC) pass and
  `scripts/check.sh` is green and the task branch is merged into `main`.
  Append the commit hash: `[x] … (abc1234)`.
- Agents may split a task into sub-tasks (T1.2a, T1.2b) or add tasks, but must
  not delete or reorder existing ones. Scope changes → ADR in DECISIONS.md.

Owner tags map to subagents in `.claude/agents/`.

## M0 — Foundations
- [x] T0.1 (architect) Run `scripts/bootstrap.sh`; fix the script until it passes on this Mac. AC: script exits 0; versions logged to STATUS.md. (d1bc490)
- [x] T0.2 (rust-core) Workspace hygiene: add `clap`, `anyhow`, `thiserror`, `tracing` + `core/deny.toml` (permissive licenses only). AC: `scripts/check.sh` green incl. `cargo deny check`. (73403db)
- [x] T0.3 (rust-core) `sonar fixtures <dir>`: generate synthetic WAV fixtures with a JSON truth manifest (sines at known notes, chords in known keys, click/drum loops at known BPMs 70–175, one-shot kick/snare/hat-like synths, noise bursts, silence, very short (<20 ms) and long (>5 min) files, stereo/mono, 16/24/32f, 44.1/48/96 kHz, plus deliberately corrupt files). AC: deterministic output (seeded); manifest documents every truth value. (6ed2e7d)
- [ ] T0.4 (swift-ui) Create `app/` with XcodeGen: SwiftUI macOS 14 app "Sonar", hardened runtime, no sandbox. AC: `xcodegen` + `xcodebuild build` succeed from `scripts/check.sh`.
- [ ] T0.5 (rust-core + swift-ui) UniFFI bridge: `sonar-ffi` crate exposing `core_version()`; script `scripts/build-core.sh` builds `SonarCore.xcframework` (arm64; x86_64 optional) + Swift bindings into `app/`. App window shows core version. AC: clean build from scratch via check.sh.

## M1 — Scan & metadata
- [ ] T1.1 (rust-core) Store: SQLite schema + migrations per ARCHITECTURE §3, `SONAR_HOME` override. AC: migration tests, WAL on.
- [ ] T1.2 (rust-core) Scanner: walk libraries (audio ext: wav aif aiff flac mp3 ogg m4a), identity via size+mtime+quick_hash, incremental rescan (added/changed/removed/moved). AC: tests on temp dirs incl. renames and unreadable files.
- [ ] T1.3 (dsp-analysis) Decoder: symphonia → mono f32, resample to 22.05 kHz (rubato), cap analysis window, report true duration. AC: all fixture formats decode; corrupt files → error, no panic.
- [ ] T1.4 (dsp-analysis) Basic meta: duration, sr, channels, bit depth, peak dBFS, integrated loudness (approx. LUFS). AC: matches fixture truth within tolerance.
- [ ] T1.5 (rust-core) Job pipeline: rayon, staged priorities (meta → DSP → embed), progress events, pause/resume/cancel, resumable after kill. AC: kill -9 mid-run then resume finishes with no duplicates.
- [ ] T1.6 (rust-core) Watcher: FSEvents via `notify`; offline volumes keep rows (`online=0`). AC: integration test add/delete/rename while running.
- [ ] T1.7 (rust-core) CLI `sonar scan|stats` + `sonar bench scan` on 50k generated files. AC: ≥ 2,000 files/s metadata scan on M4 recorded in STATUS.md.

## M2 — Text search & filters
- [ ] T2.1 (search-ranking) Tokenizer: separators, camelCase, numbers, `120bpm`/`120 BPM`, keys (`Cmin`, `C#m`, `F# minor`, `Gbmaj`), pack noise words; filename priors for BPM/key. AC: table-driven tests ≥ 60 cases.
- [ ] T2.2 (search-ranking) Tantivy index: fields name/path/tags/category/etc., synonyms map (`docs/synonyms.tsv`, editable), exclusion terms, prefix search for as-you-type. AC: unit tests + CLI `sonar search`.
- [ ] T2.3 (search-ranking) Filters/facets/sort: category, kind, BPM range, key (+ compatible keys option), duration, library, tags, favorites; facet counts. AC: tests.
- [ ] T2.4 (search-ranking) Latency: synthetic 500k-doc index, `sonar bench search`. AC: p95 < 50 ms recorded in STATUS.md.

## M3 — Audio analysis (DSP)
- [ ] T3.1 (dsp-analysis) STFT, mel filterbank, MFCC, onset strength envelope. AC: tests vs analytic signals.
- [ ] T3.2 (dsp-analysis) Aspect vectors (Spectrum, Timbre, Pitch, Amplitude, Overall) per ARCHITECTURE §5; library-level normalization stats. AC: fixed dims documented; stable across runs.
- [ ] T3.3 (dsp-analysis) Loop vs one-shot classifier (heuristics: duration, onset periodicity, tail, filename hints). AC: ≥ 95 % on fixtures.
- [ ] T3.4 (dsp-analysis) BPM for loops only (onset autocorrelation/tempogram + filename prior, octave correction). AC: ≥ 95 % (±1 BPM) on fixtures; one-shots never get BPM.
- [ ] T3.5 (dsp-analysis) Key (chroma + key profiles, filename prior) and pitch/note for tonal one-shots (YIN). AC: fixtures ≥ 90 % key, ≥ 95 % note.
- [ ] T3.6 (eval) `sonar eval`: metrics per EVAL.md over fixtures, silver set (filename-labeled real files) and golden set; JSON + markdown report into `eval/reports/`; `--check` exits non-zero on regression vs `eval/reports/BASELINE.md` (used by `scripts/check.sh`). AC: report generated; baseline written to BASELINE.md + STATUS.md.
- [ ] T3.7 [HUMAN] Founder points `SONAR_GOLDEN_DIR` at a real sample folder and labels ~300 files (CSV template from `sonar eval --init-golden`). Agents continue with silver/fixtures meanwhile.

## M4 — Similarity search
- [ ] T4.1 (search-ranking) Vector store: mmap matrices per aspect, row-id mapping, incremental append, compaction. AC: tests; 1M×(dims) load < 1 s.
- [ ] T4.2 (search-ranking) `similar` query: seed = file id or external file (analyzed on the fly), aspect weights, filters respected. AC: CLI `sonar similar`; fixtures sanity (same synth/pitch family ranks first).
- [ ] T4.3 (search-ranking) Performance: SIMD brute force vs USearch ANN + re-rank; pick by benchmark. AC: p95 < 50 ms at 500k; decision recorded as ADR.
- [ ] T4.4 (eval) Similarity metrics (P@10) in eval; tune aspect features against silver/golden. AC: numbers in STATUS.md.

## M5 — Categories & natural-language search (CLAP)
- [ ] T5.1 (dsp-analysis) Select CLAP model with license allowing commercial redistribution; export ONNX (audio + text towers), int8 if accuracy holds; download script with checksum (weights not in git). AC: ADR with model, license, size, speed.
- [ ] T5.2 (rust-core) `embed` module via `ort` (CoreML EP, CPU fallback), batched, lowest pipeline priority. AC: ≥ 100 files/s on M4 or documented best.
- [ ] T5.3 (dsp-analysis) Category classifier: zero-shot CLAP prompts + DSP/heuristic features + filename tokens, calibrated confidence; taxonomy in `docs/categories.md`. AC: fixtures + silver accuracy reported; golden ≥ 90 % when available.
- [ ] T5.4 (search-ranking) Natural-language + hybrid ranking (BM25 + CLAP + filters, RRF first). AC: NDCG@10 on text-query set in eval; no latency regression.

## M6 — macOS app MVP
- [ ] T6.1 (swift-ui) App shell: sidebar (libraries, collections, favorites), toolbar search, results, inspector; add/remove library; indexing progress (pause/resume).
- [ ] T6.2 (swift-ui) Results table: NSTableView, lazy paging from core, sortable columns (name, category, kind, BPM, key, duration, library), 1M rows smooth. AC: XCTest perf test + manual-free scroll benchmark.
- [ ] T6.3 (swift-ui) Instant search + filter chips/facets, keyboard-first (⌘F, arrows, space, return).
- [ ] T6.4 (swift-ui) Preview: AVAudioEngine, auto-play on select, pre-buffer neighbors, pitch control, loop toggle, loudness-normalized preview.
- [ ] T6.5 (swift-ui) Waveform: peaks from core, CALayer/Metal rendering, zoom, playhead.
- [ ] T6.6 (swift-ui) Similarity UI: "Find similar", aspect sliders, drop external file to search.
- [ ] T6.7 (swift-ui) Drag & drop out to DAW/Finder (file URLs), multi-select.
- [ ] T6.8 (swift-ui) Organize: favorites, collections, tags, overrides for BPM/key/category, hide.
- [ ] T6.9 (swift-ui) Duplicates view (exact + near via aspect vectors).

## M7 — Polish & release
- [ ] T7.1 (reviewer) Full perf + robustness pass (1M synthetic files, corrupt files, offline drives). Fix findings.
- [ ] T7.2 (swift-ui) Onboarding (pick folders), settings, empty states, error states.
- [ ] T7.3 (architect) Release pipeline script: universal build, codesign, notarize, DMG, Sparkle updates (keys via env). 
- [ ] T7.4 [HUMAN] Apple Developer ID, notarization credentials, Paddle/Lemon Squeezy account, pricing.
- [ ] T7.5 (swift-ui) Licensing: trial + license key validation (offline-friendly) behind a provider-agnostic interface.

## M8 — Founder acceptance (end of MVP)
- [ ] T8.1 [HUMAN] Founder tests the app end to end on his library: scan, search, similarity, preview feel, drag & drop into Ableton Live. Findings become new tasks.

## Future (do not start)
v1.x: record-to-search, 2D map, tempo-synced preview, kit builder, `.als` project map.
v2: preset search (JUCE sidecar, ADR-007). Windows (~1 year).
