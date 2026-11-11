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
- Split/add only what the AC or the MVP really needs. Follow-ups, extra perf,
  rare edge cases and non-trivial NITs go to `## Later` (end of file, one line
  each, no checkbox). The loop never picks Later items.

Owner tags map to subagents in `.claude/agents/`.

## M0 — Foundations
- [x] T0.1 (architect) Run `scripts/bootstrap.sh`; fix the script until it passes on this Mac. AC: script exits 0; versions logged to STATUS.md. (d1bc490)
- [x] T0.2 (rust-core) Workspace hygiene: add `clap`, `anyhow`, `thiserror`, `tracing` + `core/deny.toml` (permissive licenses only). AC: `scripts/check.sh` green incl. `cargo deny check`. (73403db)
- [x] T0.3 (rust-core) `sonar fixtures <dir>`: generate synthetic WAV fixtures with a JSON truth manifest (sines at known notes, chords in known keys, click/drum loops at known BPMs 70–175, one-shot kick/snare/hat-like synths, noise bursts, silence, very short (<20 ms) and long (>5 min) files, stereo/mono, 16/24/32f, 44.1/48/96 kHz, plus deliberately corrupt files). AC: deterministic output (seeded); manifest documents every truth value. (6ed2e7d)
- [x] T0.4 (swift-ui) Create `app/` with XcodeGen: SwiftUI macOS 14 app "Sonar", hardened runtime, no sandbox. AC: `xcodegen` + `xcodebuild build` succeed from `scripts/check.sh`. (f1920d3)
- [x] T0.5 (rust-core + swift-ui) UniFFI bridge: `sonar-ffi` crate exposing `core_version()`; script `scripts/build-core.sh` builds `SonarCore.xcframework` (arm64; x86_64 optional) + Swift bindings into `app/`. App window shows core version. AC: clean build from scratch via check.sh. (d7bdea5)

## M1 — Scan & metadata
- [x] T1.1 (rust-core) Store: SQLite schema + migrations per ARCHITECTURE §3, `SONAR_HOME` override. AC: migration tests, WAL on. (b6303cc)
- [x] T1.2 (rust-core) Scanner: walk libraries (audio ext: wav aif aiff flac mp3 ogg m4a), identity via size+mtime+quick_hash, incremental rescan (added/changed/removed/moved). AC: tests on temp dirs incl. renames and unreadable files. (40a1fd9)
- [x] T1.3 (dsp-analysis) Decoder: symphonia → mono f32, resample to 22.05 kHz (rubato), cap analysis window, report true duration. AC: all fixture formats decode; corrupt files → error, no panic. (320a774)
- [x] T1.3a (eval) Multi-format fixtures: FLAC via a permissive encoder (e.g. `flacenc`, Apache-2.0) and tiny self-made MP3/Ogg/M4A files (or generated at test time); decode test asserts counted duration == declared and no spurious `partial` (encoder priming/padding). AC: every scanner extension has at least one decoded fixture. (7235660)
- [x] T1.3b (eval) AAC-in-M4A fixture: hand-written silent AAC-LC frames + `elst`/`iTunSMPB` priming (2112 frames) so the most common real `.m4a` length path is tested; also a non-silent lossy check that priming is trimmed from the start. AC: decode length == declared, no `partial`. (9d4e2ad)
- [x] T1.4 (dsp-analysis) Basic meta: duration, sr, channels, bit depth, peak dBFS, integrated loudness (approx. LUFS). AC: matches fixture truth within tolerance. (7f1f9c4)
- [x] T1.5 (rust-core) Job pipeline: rayon, staged priorities (meta → DSP → embed), progress events, pause/resume/cancel, resumable after kill. AC: kill -9 mid-run then resume finishes with no duplicates. Note (T1.3): install a quiet panic hook in workers (decode's `catch_unwind` still prints the default panic message). Note (T1.4): the fast "metadata first" stage must not call `decode_with_meta` with the 600 s default meta window (long lossy files 5–17× slower; fixture set 1412 → 803 files/s): probe with a 0 s window, no meter, and run peak/loudness in the DSP stage (or lower the window), with an ADR; consider adding `BasicMeta.problem` so the reason metering stopped reaches `files.error`. (9b4417a)
- [x] T1.6 (rust-core) Watcher: FSEvents via `notify`; offline volumes keep rows (`online=0`). AC: integration test add/delete/rename while running. (990ebc7)
- [x] T1.7 (rust-core) CLI `sonar scan|stats` + `sonar bench scan` on 50k generated files. AC: ≥ 2,000 files/s metadata scan on M4 recorded in STATUS.md. Also `sonar bench decode` (from T1.3) with the number in STATUS, incl. meta on/off (from T1.4; currently an `#[ignore]` test in `tests/meta.rs`). From T1.5 review: bench `sonar analyze --stages meta` (files/s) and `--stages dsp` in release. (8db8b87)
- [x] T1.7a (dsp-analysis) (split from T1.7; T1.7a–d) Meta stage on cold libraries: it is I/O-bound (counts every packet for duration; cold clone-extent bench 2.4–5k files/s vs 28k warm) — take PCM WAV/AIFF (and other exact-header formats) duration from the header, validated against the file size so truncated files still report the real length / `partial`; measure with `sonar bench analyze --cold` (after `sudo purge` if available). AC: numbers before/after in STATUS.md; fixture durations unchanged; truncated-file tests. (a0cba00)
- [x] T1.7b (rust-core) `sonar bench watch` (from T1.6): measure watcher rescan duration / duty cycle on the 50k library during a continuous copy (whole-library rescan per event burst; scope to subtrees if too slow) and scan-vs-analyze SQLite contention (BUSY → watcher `Error` + 5 s retry). AC: numbers in STATUS.md; any change justified by before/after numbers. (9881d25)
- [x] T1.7c (rust-core) Job pipeline follow-ups (from T1.5 review): consider pipelining batches (each batch waits for its slowest file; pool idle during claim/commit) and committing finished results when paused mid-batch — decide by measurement with `sonar bench analyze`. AC: numbers in STATUS.md; any change justified by before/after numbers. (91ae8dc)
- [x] T1.7d (dsp-analysis) (from T1.7a) Exact length for truncated/over-long PCM: symphonia counts a short final PCM packet as a full 1,152 frames (a WAV cut inside its last packet over-reports by ≤ 1,151 frames, not `partial`), and ignores AIFF COMM `numSampleFrames` (SSND longer than COMM over-reports). Count PCM packets by actual bytes / honour COMM; bump the meta stage version. AC: truncated WAV/AIFF tests report the exact surviving frame count + `partial`. (fd4ae10)

## M2 — Text search & filters
- [x] T2.1 (search-ranking) Tokenizer: separators, camelCase, numbers, `120bpm`/`120 BPM`, keys (`Cmin`, `C#m`, `F# minor`, `Gbmaj`), pack noise words; filename priors for BPM/key. AC: table-driven tests ≥ 60 cases. (b7022b4)
- [x] T2.2 (search-ranking) Tantivy index: fields name/path/tags/category/etc., synonyms map (`docs/synonyms.tsv`, editable), exclusion terms, prefix search for as-you-type. AC: unit tests + CLI `sonar search`. (0040b47)
- [x] T2.3 (search-ranking) Filters/facets/sort: category, kind, BPM range, key (+ compatible keys option), duration, library, tags, favorites; facet counts. AC: tests. (ef3c1e5)
- [x] T2.4 (search-ranking) Latency: synthetic 500k-doc index, `sonar bench search`. AC: p95 < 50 ms recorded in STATUS.md. Note (T2.3): also bench browse-all + facets and range + facets (23 ms p95 at 500k in RAM) on an on-disk index. (842d09e)

## M3 — Audio analysis (DSP)
- [x] T3.1 (dsp-analysis) STFT, mel filterbank, MFCC, onset strength envelope. AC: tests vs analytic signals. (588f79f)
- [x] T3.2 (dsp-analysis) Aspect vectors (Spectrum, Timbre, Pitch, Amplitude, Overall) per ARCHITECTURE §5; library-level normalization stats. AC: fixed dims documented; stable across runs. Note (T3.1): `dsp::N_MFCC` = 20 incl. c0 (loudness-dependent); decide whether Timbre drops c0 (Amplitude covers loudness). (75739b4)
- [x] T3.3 (dsp-analysis) Loop vs one-shot classifier (heuristics: duration, onset periodicity, tail, filename hints). AC: ≥ 95 % on fixtures. (c24ef7c)
- [ ] T3.4 (dsp-analysis) BPM for loops only (onset autocorrelation/tempogram + filename prior, octave correction). AC: ≥ 95 % (±1 BPM) on fixtures; one-shots never get BPM. Note (T3.3): `classify::classify_kind` is not run by the `jobs` Dsp stage nor stored as analysis `kind` yet; wire it here (BPM gating needs it).
- [ ] T3.5 (dsp-analysis) Key (chroma + key profiles, filename prior) and pitch/note for tonal one-shots (YIN). AC: fixtures ≥ 90 % key, ≥ 95 % note.
- [ ] T3.6 (eval) `sonar eval`: metrics per EVAL.md over fixtures, silver set (filename-labeled real files) and golden set; JSON + markdown report into `eval/reports/`; `--check` exits non-zero on regression vs `eval/reports/BASELINE.md` (used by `scripts/check.sh`). AC: report generated; baseline written to BASELINE.md + STATUS.md.
- [ ] T3.7 [HUMAN] Founder points `SONAR_GOLDEN_DIR` at a real sample folder and labels ~300 files (CSV template from `sonar eval --init-golden`). Agents continue with silver/fixtures meanwhile.

## M4 — Similarity search
- [ ] T4.1 (search-ranking) Vector store: mmap matrices per aspect, row-id mapping, incremental append, compaction. AC: tests; 1M×(dims) load < 1 s. Note (T3.2): the `jobs` Dsp stage does not compute aspect vectors yet; wire `AspectExtractor` (raw vectors stored) + `LibraryNorm` (rayon fold/merge, persisted) into the pipeline here.
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

## Later (not picked by the loop — founder triages after M8)
- (from T1.7c review) Resume pressed while a failed job is shutting down may start a few files that are stopped immediately.
- (from T1.7c review) Loop guard against a loop that cannot happen today.
- (from T1.7c) `cargo deny` advisories check failed once (not in check.sh, no deps changed); cause unconfirmed. (T1.7d: it is `basic-toml` unmaintained via `uniffi-bindgen`; ignore in `deny.toml` with a reason or bump.)
- (from T1.7d) An intact AIFF whose writer over-declares `COMM` `numSampleFrames` is now `partial`; check AIFF `partial` counts on a real library / eval.
- (from T2.1 review) Key prior for one-shots ending in a bare note letter (`KSHMR_Kick_08_C`, `Bass_C`) when the name has a tonal word (bass/808/pluck/lead/keys).
- (from T2.1 review) `Fm_Bass_Growl` gets key prior F minor though "Fm" often means FM synthesis; check on silver set.
- (from T2.1 review) All-lowercase names (`vox_chop_am_120`, `rnb_keys_loop_gm_90`) get no key prior; consider allowing with explicit BPM / loop word.
- (from T2.1 review) `-6dB` emits stray `d`/`b` tokens from the camelCase split.
- (from T2.1 review) Full-width digits (`１２０`) dropped as separators; NFKC folding would cover them.
- (from T2.2) Synonym expansion for unfinished (prefix) words (`kic` → kick group), guarded so `bas` doesn't pull in `bassdrum`.
- (from T2.2 review) `sonar scan` rebuilds the whole text index; use `refresh_files` with only the changed ids (check against the 500k budget in T2.4).
- (from T2.2 review) `sonar watch` does not refresh the text index yet; wire `refresh_files` into the watcher.
- (from T2.2) Text search is not cancellable yet (ARCHITECTURE §6); add with the FFI search API.
- (from T2.2 review) Any index open error deletes and rebuilds the index dir; only do so on real corruption, not transient errors (EMFILE, permissions).
- (from T2.2 review) `Error::Index(tantivy::TantivyError)` leaks a Tantivy type into the public API; store a `String` before FFI.
- (from T2.2 review) Single-letter earlier words (`c mi` on the way to "c minor") still require the unfinished last word; results can blink empty while typing.
- (from T2.2 review) Add an assertion that unfinished `c#` finds a `C#` file (proves regex escaping in `search/build.rs`).
- (from T2.3) Natural sort of names (`Kick 2` before `Kick 10`); ties in field sorts fall back to name, not file id.
- (from T2.3) BPM and duration histogram facets for range sliders.
- (from T2.3) A "no value" facet bucket and a filter for unknown category/key.
- (from T2.3) Optional half-time / double-time BPM matching.
- (from T2.3) Show BPM, key and duration in `sonar search` result rows.
- (from T2.3) Tag facet labels are lowercased; show the user's spelling from the `tags` table.
- (from T2.3 review) Facet dense counters allocate up to 64k entries per dimension per segment per search; reuse/cap if T2.4 shows pressure.
- (from T2.3 review) Non-numeric BPM override (`"128 bpm"`) becomes unknown with no prior fallback; override writer must store plain numbers.
- (from T2.3 review) Future favourite/hidden/override/tag setters must call `TextIndex::refresh_files` (index only reflects them after refresh/rebuild).
- (from T2.4) 1-letter prefix + facets is the as-you-type tail (~32 ms on M4 Pro); measure on a base M1, consider 2-char minimum prefix or no facets for 1-letter queries.
- (from T2.4) Browse-all + facets is a flat ~27–29 ms full scan; cache browse-all facet counts per index generation if slower Macs near the budget.
- (from T2.4) Bench first query is only cold-ish (OS cache warm); add an optional truly cold run (`purge` / fresh clone) for app-start latency.
- (from T2.4) Full 500k index rebuild = 1.8 s; use it when deciding the T2.2 scan-rebuild vs `refresh_files` item.
- (from T2.4 review) Browse classes have 3 queries (p95 ≈ max of 30 runs); raise default rounds or add queries. No test for the budget-miss non-zero exit (move check into core + unit test).
- (from T3.1 review) `FeatureMatrix` has public fields; add a checked constructor so `data.len() == n_frames * n_cols` holds.
- (from T3.1 review) DSP timing is only an `#[ignore]` test; add `sonar bench dsp` once T3.2 wires DSP into the pipeline.
- (from T3.2) serde_json's default float parsing can be 1 ulp off; enable `float_roundtrip` (or store stats in binary) if persisted `LibraryNorm` must reproduce z-scores bit-exactly.
- (from T3.3 review) Loop/one-shot weights are hand-tuned on synthetic fixtures; re-check on the silver/golden eval (T3.6/T3.7). Weakest fixture: 1.9 s loop at p_loop 0.70.
- (from T3.3 review) Classifier timing is only an `#[ignore]` test; add it to `sonar bench` when wired into the pipeline.
- (from T3.3 review) Hint word `beat` tags "Beat Kits/" folders (mostly one-shots) as loop (±2 logit); consider dropping it.
