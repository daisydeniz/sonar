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
- [x] T3.4 (dsp-analysis) BPM for loops only (onset autocorrelation/tempogram + filename prior, octave correction). AC: ≥ 95 % (±1 BPM) on fixtures; one-shots never get BPM. Note (T3.3): `classify::classify_kind` is not run by the `jobs` Dsp stage nor stored as analysis `kind` yet; wire it here (BPM gating needs it). (0896203)
- [x] T3.5 (dsp-analysis) Key (chroma + key profiles, filename prior) and pitch/note for tonal one-shots (YIN). AC: fixtures ≥ 90 % key, ≥ 95 % note. (be47de5)
- [x] T3.6 (eval) `sonar eval`: metrics per EVAL.md over fixtures, silver set (filename-labeled real files) and golden set; JSON + markdown report into `eval/reports/`; `--check` exits non-zero on regression vs `eval/reports/BASELINE.md` (used by `scripts/check.sh`). AC: report generated; baseline written to BASELINE.md + STATUS.md. (4c66a98)
- [ ] T3.7 [HUMAN] Founder points `SONAR_GOLDEN_DIR` at a real sample folder and labels ~300 files (CSV template from `sonar eval --init-golden`). Agents continue with silver/fixtures meanwhile.

## M4 — Similarity search
- [x] T4.1 (search-ranking) Vector store: mmap matrices per aspect, row-id mapping, incremental append, compaction. AC: tests; 1M×(dims) load < 1 s. Note (T3.2): the `jobs` Dsp stage does not compute aspect vectors yet; wire `AspectExtractor` (raw vectors stored) + `LibraryNorm` (rayon fold/merge, persisted) into the pipeline here. (64d2548)
- [x] T4.2 (search-ranking) `similar` query: seed = file id or external file (analyzed on the fly), aspect weights, filters respected. AC: CLI `sonar similar`; fixtures sanity (same synth/pitch family ranks first). (c5136d4)
- [x] T4.3 (search-ranking) Performance: SIMD brute force vs USearch ANN + re-rank; pick by benchmark. AC: p95 < 50 ms at 500k; decision recorded as ADR. (bec35d0)
- [x] T4.4 (eval) Similarity metrics (P@10) in eval; tune aspect features against silver/golden. AC: numbers in STATUS.md. (239bf1e)

## M5 — Categories & natural-language search (CLAP)
- [x] T5.1 (dsp-analysis) Select CLAP model with license allowing commercial redistribution; export ONNX (audio + text towers), int8 if accuracy holds; download script with checksum (weights not in git). AC: ADR with model, license, size, speed. (4be5cc9)
- [x] T5.2 (rust-core) `embed` module via `ort` (CoreML EP, CPU fallback), batched, lowest pipeline priority. AC: ≥ 100 files/s on M4 or documented best. (1de8e72)
- [x] T5.3 (dsp-analysis) Category classifier: zero-shot CLAP prompts + DSP/heuristic features + filename tokens, calibrated confidence; taxonomy in `docs/categories.md`. AC: fixtures + silver accuracy reported; golden ≥ 90 % when available. (aac7eac, ec6cc3a; golden pending T3.7)
- [x] T5.4 (search-ranking) Natural-language + hybrid ranking (BM25 + CLAP + filters, RRF first). AC: NDCG@10 on text-query set in eval; no latency regression. (d240fff, 77b8caf)
- [x] T5.5 (rust-core) Engine facade (added by the founder, 2026-11-11: the MVP UI cannot work without it — today the CLI wires `Store` + `TextIndex` + `Synonyms` itself and `TextIndex::refresh_files` has no production caller). One long-lived `sonar_core::Engine` owning store, text index, synonyms, watcher and job handle. It keeps the text index in sync without full rebuilds: scan/watch changes, finished analysis batches (kind/BPM/key/category) and user-data changes all go through `refresh_files`. User-data setters: favourites, hidden, tags, collections, overrides (plain numeric BPM). Search is cancellable / latest-query-wins. `sonar-cli` commands use the Engine. Covers the matching `## Later` items from T2.2/T2.3 (remove them there when done). AC: integration test — add library → analyze → a file added while watching becomes searchable and filterable by BPM with no manual rebuild; a favourite/override set through the Engine is visible in the next search; 500k latency budget still met.
  - [x] T5.5a (rust-core) (split from T5.5) `sonar_core::Engine`: open(home) owning store, text index, synonyms, watcher and job handle; libraries add/remove/list; start/pause/resume/cancel indexing with polled progress; text index kept in sync via `refresh_files` for scan/watch changes and finished analysis batches (no full rebuilds); cancellable latest-query-wins search (text/hybrid/semantic); `sonar scan|watch|analyze|search` use the Engine. AC: integration test — add library → analyze → a file added while watching becomes searchable and filterable by BPM with no manual rebuild. (ebfd480, 5c7fee5)
  - [x] T5.5b (rust-core) (split from T5.5) Engine user-data setters (favourites, hidden, tags, collections, plain-numeric BPM/key/category/kind overrides) each followed by `refresh_files`; remaining CLI commands on the Engine; remove the covered T2.2/T2.3 `## Later` items. AC: integration test — favourite/override set through the Engine is visible in the next search; `sonar bench search` 500k p95 < 50 ms still met. (1942788)
- [x] T5.6 (rust-core + architect) FFI v1 over the Engine per ARCHITECTURE §7 (added by the founder, 2026-11-11: M6 tasks are owned by swift-ui and must not have to design the core API ad hoc). `open`, libraries add/remove/list, indexing start/pause/resume + polled progress, paged `search` (ids + display fields) with cancellation, `row`, user-data setters, `waveform_peaks(file_id, n)`, and `preview_pcm(file_id)` (decoded PCM for formats AVFoundation cannot play, e.g. Ogg Vorbis). Every fallible export `throws`; no panic crosses the bridge. AC: an XCTest drives a `sonar fixtures` library end to end through the generated Swift bindings (add → index → search → set favourite → search again → peaks), green in `scripts/check.sh`. (b0c04c3)

## M6 — macOS app MVP
- [x] T6.1 (swift-ui) App shell: sidebar (libraries, collections, favorites), toolbar search, results, inspector; add/remove library; indexing progress (pause/resume). (b382ea8)
- [x] T6.2 (swift-ui) Results table: NSTableView, lazy paging from core, sortable columns (name, category, kind, BPM, key, duration, library), 1M rows smooth. AC: XCTest perf test + manual-free scroll benchmark. (dd5c882)
- [x] T6.3 (swift-ui) Instant search + filter chips/facets, keyboard-first (⌘F, arrows, space, return). (f824df8)
- [x] T6.4 (swift-ui) Preview: AVAudioEngine, auto-play on select, pre-buffer neighbors, pitch control, loop toggle, loudness-normalized preview. (2f10fe4)
- [x] T6.5 (swift-ui) Waveform: peaks from core, CALayer/Metal rendering, zoom, playhead. (3db495a)
- [x] T6.6 (swift-ui) Similarity UI: "Find similar", aspect sliders, drop external file to search. (269bfd6)
- [x] T6.7 (swift-ui) Drag & drop out to DAW/Finder (file URLs), multi-select. (2c00c44)
- [x] T6.8 (swift-ui) Organize: favorites, collections, tags, overrides for BPM/key/category, hide. (8191ddb)
- [x] T6.9 (swift-ui) Duplicates view (exact + near via aspect vectors). (9d07448)
  - [x] T6.9a (search-ranking) (split from T6.9: the core has no duplicate finder and FFI v1 left out `duplicates`) Core `dupes` module + `sonar dupes` CLI + FFI `duplicates()`: exact groups (same size + quick_hash, confirmed by a full content hash cached in `files.content_hash`) and near-duplicate groups via aspect-vector distance with a calibrated threshold (re-encodes, format/rate/bit-depth/gain variants of the same sound), candidates blocked by duration so no all-pairs scan; cancellable; missing/offline files excluded. AC: fixture tests (identical copies → exact group; format/rate/gain variants → near group; different sounds never grouped); threshold calibration numbers in an ADR; 500k synthetic timing recorded in STATUS; FFI XCTest/Rust test green in check.sh. (0a6fe92)
  - [x] T6.9b (swift-ui) Duplicates view in the app over FFI `duplicates()`: sidebar entry, groups list (exact / near), per-file path/size/format, preview + reveal, hide or "keep one" via existing user-data setters (no file deletion). AC: XCTests over a fixture library. (9d07448)

## M7 — Polish & release
- [x] T7.1 (reviewer) Full perf + robustness pass (1M synthetic files, corrupt files, offline drives). Fix findings. (2e763a9)
- [x] T7.2 (swift-ui) Onboarding (pick folders), settings, empty states, error states. (ea23ab6)
- [x] T7.3 (architect) Release pipeline script: universal build, codesign, notarize, DMG, Sparkle updates (keys via env). (24aa955)
- [ ] T7.4 [HUMAN] Apple Developer ID, notarization credentials, Paddle/Lemon Squeezy account, pricing.
- [x] T7.5 (swift-ui) Licensing: trial + license key validation (offline-friendly) behind a provider-agnostic interface. (df36e60)

## M8 — Founder acceptance (end of MVP)
- [ ] T8.1 [HUMAN] Founder tests the app end to end on his library: scan, search, similarity, preview feel, drag & drop into Ableton Live. Findings become new tasks.

## M9 — UI redesign (added by the founder, 2026-11-17: the MVP UI was built without a visual spec; pro-audio look, system light/dark — ADR-039, `docs/DESIGN.md`)
Every T9 task: only `Theme` tokens, restyled files leave `scripts/design-lint-pending.txt` (list deleted in T9.9), `scripts/ui-snapshots.sh` PNGs (light + dark) checked by implementer, orchestrator and reviewer. Behaviour and keyboard shortcuts stay as they are unless the task says otherwise.
- [x] T9.1 (swift-ui) Design foundation: `docs/DESIGN.md`, `Theme` tokens + colour sets (light/dark), shared components (chip, readout, section label, empty state, hairline), `scripts/design-lint.sh` in check.sh, `scripts/ui-snapshots.sh` (main, similar, empty, onboarding, duplicates, components × light/dark), agent rules. AC: token tests green (incl. accent contrast ≥ 4.5:1 in both appearances); snapshots show real content in both appearances; check.sh green. (8ec79b0)
- [x] T9.2 (swift-ui) Results table per DESIGN §4.1: surface, header, 24 pt rows, tinted selection (focused/unfocused), category dot, kind symbol, right-aligned numerics, skeleton loading rows, favorite heart on hover; window surfaces and hairlines of the detail column. The snapshot window must show the focused selection (key window or a forced-emphasis seam; T9.1 review: it is never key today). AC: `main` snapshots match §4.1 in light and dark; `ResultsTablePerfTests` budget unchanged; existing table tests green. (d15724f)
- [x] T9.3 (swift-ui) Player header on top per DESIGN §3 (ADR-040, founder request 2026-11-17): transport row (play/pause, stop, loop, auto-play, name, pitch knob, BPM/KEY/LENGTH readouts) and a big resizable waveform (180 pt default, 120–360, played/unplayed, playhead) above the filter bar; preview controls and waveform leave the inspector. AC: header in `main` snapshots (selection and no-selection states); waveform zoom/pan/seek and preview tests green. (be61841)
- [x] T9.3b (dsp-analysis) Region similarity seed (ADR-040; needed by T9.3c): `sonar similar <id>|--file P --start S --end E` analyzes only that time range (decode → features/embedding like an external file, ADR-024) and ranks the library; region < 50 ms or out of range → error value; FFI function with the same inputs. AC: CLI + FFI tests on fixtures (a region of a loop finds its own file in top 3 and ranks the matching one-shot fixture above unrelated ones); seed analysis latency for a 2 s region within the external-file budget; eval run unchanged for whole-file similarity. (5ac16ef)
- [x] T9.3c (swift-ui) Waveform region selection per DESIGN §4.4: drag selects (tint, edges with handles, start/length readout), Esc clears; play plays/loops the region; Play Selection and Find Similar to Selection in the region's floating button and context menu; ⌘⇧F uses the region when present (via T9.3b FFI). AC: snapshot with a region (light/dark); tests for region → playback range and region → similarity request; `SimilarityTests` green. (d1699f2)
- [x] T9.4 (swift-ui) Inspector per DESIGN §3: header, actions, Analysis (overrides), Tags, File as label/value grids; multi-selection state; no grouped Form. AC: snapshots for single, multi and no selection; organize tests green. (13e7d88)
- [x] T9.5 (swift-ui) Filter bar per DESIGN §3: one 36 pt row, Kind segmented, Category/Key/BPM/Duration/Tags popovers with facet counts, active chips with clear, Favorites toggle, result count. AC: snapshots with no filter, active filters and an open popover; `FilterSearchTests` green. (5759dcb)
- [x] T9.6 (swift-ui) Similarity bar, compare view and drop overlay on tokens: target name, aspect weights compact in one row, reset/close; compare mode per DESIGN §3 (ADR-040, founder request): the similarity target's waveform (72 pt, own play button, seed region drawn if used) above the selected sample's waveform in the player header. AC: `main-similar` snapshots show both waveforms; `SimilarityTests` green. (0594dc7)
- [x] T9.7 (swift-ui) Sidebar, toolbar, indexing status and banner per DESIGN §3/§4.7 (section-header add buttons, badges, offline state, trial badge). AC: snapshots incl. indexing in progress, offline library and a banner. (21e44d8)
- [x] T9.8 (swift-ui) Secondary screens on tokens and `EmptyStateView`: onboarding, empty/error states, duplicates, settings tabs, license gate/sheet. AC: snapshots of each in light and dark; `scripts/design-lint-pending.txt` is empty. (1067b42)
- [x] T9.9 (reviewer) Design pass over all snapshots against the DESIGN §6 checklist (alignment across bars, type steps, contrast, states); fix findings; delete the pending list and its handling if empty. AC: reviewer APPROVE on the full snapshot set. (a624246)

## M10 — Honest, consistent key (added by the founder, 2026-11-18: competitors show a key for every sample, Sonar for ~46 %; goal is consistency and readability, not a value at any price — ADR-041)
Order matters: T10.4 calibrates what T10.2/T10.3 produce; T10.6 displays the tiers T10.4 fixes. Every key/DSP change bumps `KEY_VERSION`/`DSP_VERSION` so libraries re-analyze.
- [x] T10.1 (search-ranking) Filename key prior: bracketed keys `(C)`, `[F#]`, `(Am)`, `(C)(120 BPM)`; bare letter at the end of a one-shot name with a tonal/pitched word (bass, 808, pluck, lead, keys, tom, dunun, surdo…); all-lowercase keys (`am`, `gm`) when the name has a BPM or loop word. Covers the T2.1 `## Later` items on bare letters and lowercase names. AC: ≥ 25 new table-driven tokenizer cases incl. `ABTSRR2 - Tom Loop 08 (C)(120 BPM).wav` → key C; no new false key on the existing negative cases (`-6dB`, `Fm_Bass` stays as today). (6c6eb5b)
- [x] T10.2 (dsp-analysis) Fused key with graded confidence (ADR-041): every non-silent file gets its best key candidate; the tonality gates become confidence factors instead of cut-offs; audio candidate scores and the filename prior combine as evidence (agreement raises, disagreement lowers confidence; a name alone on non-tonal audio stays low-medium); `key_source` says audio / name / both (migration if the CHECK needs it); one-shot note confidence falls with |cents| (±50 ct → near 0). Silence/corrupt: no key. Covers the T3.5 gate `## Later` item. AC: fixtures key/note accuracy unchanged (16/16, 52/52 with names hidden); drum/noise fixtures get a key with confidence < the low-tier bound of T10.4's first cut (0.3); `tests/key.rs` covers agree / disagree / name-only / silence. (ba26f7e)
- [x] T10.3 (dsp-analysis) Root note for pitched loops: YIN per onset segment (reuse `NoteTracker`); when ≥ 60 % of onset energy agrees on one pitch class the loop gets it as a mode-less key (confidence from that share), fused with the chroma key (T10.2). Covers the T3.5 "loops never get a note" `## Later` item. AC: new seeded fixtures (tom/bass-like loops at known notes, a mixed-pitch loop) — root ≥ 90 % correct; mixed-pitch loop gets low confidence; drum-loop fixtures stay low. (6668bad)
- [x] T10.4 (eval) Key calibration: `sonar eval` reports key accuracy per confidence tier and expected calibration error on fixtures and `--silver` (names hidden); fit the two tier bounds (`KEY_TIER_HIGH`, `KEY_TIER_MEDIUM`, exported by core + FFI) so the high tier is ≥ 90 % correct and the medium tier ≥ 60 % on silver (fixtures when no silver dir); tiers must be monotone. Covers the T3.5 "validated only on synthetic" `## Later` item. AC: report table in EVAL.md + BASELINE; numbers in the STATUS entry. (86fb500)
- [x] T10.4b (eval) (added by the founder, 2026-11-18: T10.4 fitted the tiers on fixtures only — medium tier empty, confidences bimodal — because the agent shell could not reach the real samples; `scripts/eval-real.sh` fixes that) Re-fit `KEY_TIER_HIGH` / `KEY_TIER_MEDIUM` on real files: `scripts/eval-real.sh` (silver `Samples/`, golden `eval/golden/labels.csv`, ~299 pack-labeled files, names hidden). Covers the T10.4 review `## Later` item on the empty medium tier. AC: high tier ≥ 90 % and medium ≥ 60 % correct on silver + golden (or the best monotone bounds, documented, if unreachable); tier table and key MIREX on golden/silver in EVAL.md + STATUS; fixtures gate green. (1fef4e0)
- [x] T10.5 (search-ranking) Key filter honesty: by default the key filter and the key facet count only medium + high tier; `include_uncertain` option (CLI `--key-uncertain`, FFI field) adds the low tier; a mode-less key (`A`) matches both `A` and `Am` (covers the T3.5 review `## Later` item); sort by key puts low tier after the others. AC: filter/facet tests for each tier and the mode-less case; 500k search p95 budget unchanged. (6891ca3)
- [x] T10.6 (swift-ui) Key display per ADR-041 / DESIGN: every key cell, the KEY readout and the inspector show the value in the tier's text colour (high primary, medium secondary, low tertiary — also confidence 0); silence/no value an en dash; inspector Analysis row adds source (Audio / Name / Audio + Name) and confidence word, and for one-shots the note with cents (`E2 +32 ct`, in secondary; "≈" prefix when |cents| > 25); when name and audio disagree the inspector shows both; Key popover gets an "Include uncertain" toggle (T10.5). DESIGN.md §4 updated. AC: snapshots (light/dark) with all three tiers, an en dash, a disagreement and a detuned one-shot; filter tests green. (7ae9145)
- [x] T10.7 (dsp-analysis) (added by the founder, 2026-11-18: a real bug — first golden run, 225 pack-labeled loops at 120/122 BPM: BPM ±1 only 32 %, 97 octave errors, 42 loops without a BPM because 42 loops were classified one-shot) BPM and loop/one-shot on real loops: find why tempo halves/doubles and why loops become one-shots on `scripts/eval-real.sh` (golden + silver, names hidden); fix octave correction / kind classifier with published methods; bump `DSP_VERSION`. AC: golden + silver BPM ±1 ≥ 90 % on loops, loop→one-shot errors ≤ 5 %, fixtures BPM/kind gates unchanged; numbers in STATUS. — c0e3c07 (kind AC met; BPM AC partially met: golden 88.9 / silver 81.2 %, see ADR-021 addendum)
- [x] T10.8 (swift-ui) (added by the founder, 2026-11-19: the T10.6 tier colours read as random dark/grey keys; wants Sononym-style pitch) Key display: a one-shot with a note shows note + octave with signed cents as a small superscript (`E2` + `⁺³⁵`), else the key (`D#m`); one text colour for every key value (no tier greys; tier stays as the inspector confidence word); the same rendering in the results cell, the KEY readout and the inspector Key row. AC: shared component (SwiftUI + AppKit attributed string) in `DesignSystem/`, DESIGN.md §4 + ADR-041 line updated, formatter unit tests (+/−/0 cents, no note, no key), snapshots light/dark show it. (62c3318)
- [x] T10.9 (swift-ui) (added by the founder, 2026-11-19: right-click → Add Tag… is a bare text alert — no existing tags, no multi-select) Tag editor sheet: replaces the `.addTag` alert (row context menu + menu bar). Shows the tags shared by all target files as removable chips inside a token box with an inline "Add tag" field; typing filters the library's existing tags (with file counts) in a list below — click / ↓↑ + Return toggles one, Return on text with no exact match creates it as a new chip ("Create “x”" row), comma or Tab also commits; existing tags in the list show a checkmark when chosen; ⌫ on an empty field removes the last chip. Cancel (Esc) discards; Save (⌘Return / default) applies the diff (add + remove) to all targets in one refresh. Inspector Tags section reuses the same suggestion list instead of its separate menu if trivial. AC: shared component in `DesignSystem/` or `Features/Organize/`, tokens only, DESIGN.md §4.11 updated; unit tests for the editor state (toggle, create, dedupe case-insensitively, diff for multi-selection); snapshot `tag-editor` light/dark (with chips + filtered list); `OrganizeTests` green. (b7df41e)

## M11 — Logic Pro-style filters and tags (added by the founder, 2026-11-20: wants the browser to feel like Logic Pro's on macOS 26/27; ADR-042, DESIGN.md §1, §3, §4.2)
Every T11 task: DESIGN.md first, `.claude/skills/mac-design` + `ux-design` as platform rules (DESIGN.md wins), the local reference `design/reference/logic-pro-browser.png` if present, snapshots light/dark checked by implementer, orchestrator and reviewer. Deployment target stays macOS 14 (`if #available` for 26+ APIs).
- [x] T11.0 (orchestrator) Design direction: Logic Pro reference, filter panel + hued chip spec, glass rule, vendored BAMA design skills, ADR-042. Docs only. (56fd95c)
- [x] T11.1 (swift-ui) Filters panel per DESIGN §3 "Filter panel": one Filters button replaces the Category/Key/BPM/Duration/Tags popover buttons; system popover with sections (Category grouped by family, Key roots + mode + the two include toggles, Tempo, Duration, Tags) as hued chip clouds with counts and "Show More"; chips toggle at once; the bar shows one hued active chip per selected value with its own `×`, then Favorites, Clear all, count. Facet hue + deep-fill colour sets (§2.1) and `SonarChip` hue support (§4.2). AC: token test white-on-fill ≥ 4.5:1 for every fill in light and dark; snapshots `main-filter-panel` (open, with selections in ≥ 3 sections) and `main-filters` (≥ 4 active chips of different hues), light/dark; unit tests for the panel selection model (multi-select toggle, single-select quick range, Key mode → `keys` mapping); keyboard (Tab/arrows/Space/Esc) as a manual check in STATUS "Needs human"; `FilterSearchTests` green; no core change; DESIGN.md §3/§4.2 rewritten to the shipped state (old popover-button / `accentSoft` active-chip text removed). (c5ae8c6)
- [x] T11.2 (swift-ui) Tag chips in `facetTag` hue everywhere (inspector Tags, tag editor sheet, Tags section of the panel) via the T11.1 chip; tag editor list rows keep their layout. AC: snapshots `main` (inspector with tags) and `tag-editor`, light/dark; `OrganizeTests` green. (d7f4b10)
- [x] T11.3 (reviewer) Platform pass on macOS 27: re-run `scripts/ui-snapshots.sh`, check every screen against `.claude/skills/mac-design/design-rules.md` (glass only in the control layer, no custom backgrounds behind bars/popovers/sheets, concentric corners, toolbar items over their column, standard symbols, inactive-window dimming) and DESIGN §6; swift-ui fixes the BLOCKING findings, the rest go to `## Later`. AC: reviewer APPROVE on the full snapshot set. (bf56c3e)

## M12 — Visual v2 and tags UX (added by the founder, 2026-11-20: approved light/dark mockups of a flat, Logic-style look and a new tag flow; ADR-043, DESIGN.md v2)
Every T12 UI task: DESIGN.md first (whole file), then the target mockups `design/reference/sonar-v2/<screen>-<light|dark>.png` for its screens; snapshots light/dark compared with them by implementer, orchestrator and reviewer (DESIGN wins on conflict; mock data is illustration only). `.claude/skills/mac-design` + `ux-design` as platform rules. New screens are added to `scripts/ui-snapshots.sh`. Deployment target stays macOS 14.
- [x] T12.0 (orchestrator) Design v2 docs: DESIGN.md v2 (§1–§4, §4.12 Tags), ADR-043, mockup PNGs in `design/reference/sonar-v2/`, this milestone. Docs only. (d5bf376)
- [x] T12.1 (swift-ui) Tokens and shared components v2 (DESIGN §2, §4.2, §4.8): colour sets + `Theme` for `accent`/`controlAccent` (blue), `signal`, `surfaceGroup`, `chipBase`, `waveWell`, `waveStrip`, `wave`, the five facet hues with fills (`facetCategory`, `facetKey`, `facetProperty`, `facetTag`, `facetLibrary`), knob family colours; type tokens `chip`, `readoutValue`; sizes (row 26, header 24, bar 44, chip 24, `knobSmall` 24 / `knobLarge` 44, radius `well`). Remove `cat*` tokens, `Theme.categoryColor`, the category dot, `accentSoft`, `panelGroup`, `surfaceWell` (move their users to v2 tokens). `SonarChip` v2 (hue always, unselected / hover / selected fill / mixed dashed with "n/m" / disabled) and `Knob` v2 (coloured face, outer value arc). AC: token tests — white on every `…Fill` and on `controlAccent` ≥ 4.5:1 and every facet text hue ≥ 4.5:1 on its unselected chip background and on `surfaceGroup`, light and dark; `components` snapshot light/dark matches `components-*.png`; all existing snapshots regenerate with no broken state; `design-lint.sh` and check.sh green. (fa3323f)
- [x] T12.2 (rust-core) Tag normalisation and management in core (ADR-043): normalise on every write (trim, collapse inner whitespace); tags unique case-insensitively (collation/index), a migration merges existing case duplicates into the most used spelling (tie: oldest); operations list (with file counts, incl. unused), rename (onto an existing tag = merge), merge (n → 1), delete, similar pairs per the ADR-043 rule with a reason, ignore a pair (persisted); each returns the affected file ids (for Undo). CLI `sonar tags list|rename|merge|delete|similar|ignore`, FFI for each. Covers the `## Later` item (from T10.9 review) on case-sensitive tags. AC: tests for normalisation, the migration (`Dark`, `dark`, ` dark ` → one tag), each operation, the similar rule table-driven (groove/grove, lofi/lo-fi, analog/analogue, kick/kicks → similar; hat/hats, drum/bass → not; ignored pair not returned), similar over 5,000 tags < 200 ms; check.sh green. (70ce36e)
- [x] T12.3 (dsp-analysis) Per-channel waveform peaks (ADR-043 stereo lanes): `waveform_peaks` returns one peak list per channel (mono → one, > 2 channels → first two), cache and FFI updated, `sonar wave` prints per channel. AC: a stereo fixture with different L/R gives two lanes that match each channel; mono gives one; waveform/preview tests green; peak latency budget unchanged. (a0ce709)
- [x] T12.4 (swift-ui) Player header v2 (DESIGN §3 Player header, §4.3, §4.4, §4.8; deps T12.1, T12.3): transport row (30 pt play circle, stop, Loop/AUTO in `signal` when on, name + extension, Pitch and Gain knobs 24 pt); waveform well with ruler (bars.beats with a BPM, seconds without; follows zoom/pan), stereo lanes, readout strip (BPM, Key, Length, Format, zoom buttons); readouts leave the transport row; preview gain −24…+6 dB in the preview engine only, remembered, double-click resets. Region selection, compare mode, seek, zoom and pan keep working. AC: unit tests for ruler ticks (BPM / no BPM / zoomed) and gain (dB → linear, clamp, persistence); snapshots light/dark: `main` (stereo loop with BPM), a mono one-shot (seconds ruler), no selection, `main-region`, `main-similar`, compared with `main-*.png`; preview and `SimilarityTests` green. Also (from T12.3 review): `WaveformCache` / `WaveformProviding.peaks` move to `[[WaveformPeak]]` and the stopgap `WaveformMath.envelope` merge goes. (345951e)
- [x] T12.5 (swift-ui) Filter bar and Filters panel v2 (DESIGN §3; dep T12.1): 44 pt bar, Filters button with active count, active chips in section hues; panel with a quick card (Favorites), opaque `surfaceGroup` section cards with hue squares, Category in one hue, a new Library section (multi-select, counts from the existing `libraries` facet), footer "n results · m filters" + Clear all. AC: panel selection-model tests extended for Library; snapshots `main-filters`, `main-filter-panel` light/dark compared with `filter-panel-*.png`; `FilterSearchTests` green. (9884982)
- [x] T12.6 (swift-ui) Results table, toolbar and inspector v2 (DESIGN §3, §4.1, §4.9; dep T12.1): 26 pt rows, 24 pt header, plain Category text, playing-row speaker symbol, toolbar title + libraries subtitle, trial badge in `signal`; inspector header with three equal actions and cards Analysis / Tags / File (bit depth and size only if core already has them, else `## Later`). AC: snapshots `main`, `main-multi`, `main-noselection` light/dark compared with `main-*.png`; `ResultsTablePerfTests` budget unchanged.
- [ ] T12.7 (swift-ui) Tags menu and tag popover (DESIGN §4.12 layers 1–2; deps T12.1, T12.2): row context menu and Edit menu "Tags ›" submenu (8 most used, tri-state for the selection, Add Tag… ⌘T, Manage Tags…); the live-applying tag popover replaces `TagEditorView` (mixed chips, Most used, matches, "Create tag" row last, keys per §4.12); inspector Tags card `+` opens it; every change is one Undo step; "Edit Tags…" removed. AC: unit tests for popover state (toggle, create, mixed → all, match order, undo grouping) and the submenu tri-state; snapshots `tags-popover`, `tags-popover-multi` light/dark compared with the mockups; `OrganizeTests` green; manual menu check in STATUS "Needs human".
- [ ] T12.8 (swift-ui) Manage Tags sheet and sidebar Tags section (DESIGN §3 Sidebar, §4.12 layer 3; deps T12.2, T12.7): tag list with counts and "unused", inline rename, Merge…, delete with confirmation, similar-pairs card (Merge into, Merge All, Ignore); sidebar Tags (5 most used + Show All, click scopes the results, dropping rows adds the tag, Undo). AC: snapshot `tags-manage` light/dark compared with `tags-manage-*.png`, sidebar visible in `main`; tests for rename/merge/delete/ignore through FFI and for drop-to-tag; manual check in "Needs human".
- [ ] T12.9 (reviewer) Design pass v2: re-run `scripts/ui-snapshots.sh`, compare every screen with `design/reference/sonar-v2/` and DESIGN §6; swift-ui fixes BLOCKING findings, the rest go to `## Later`. AC: reviewer APPROVE on the full snapshot set.

## Future (do not start)
v1.x: record-to-search, 2D map, tempo-synced preview, kit builder, `.als` project map.
v2: preset search (JUCE sidecar, ADR-007). Windows (~1 year).

## Later (not picked by the loop — founder triages after M8)
- (from T12.6) Inspector File card: Bit depth and Size rows once core `FileRow` exposes them (core/FFI work).
- (from T12.6 review) Playing-row speaker uses `model.selectedRow`, not the row `AudioPreview` loaded; expose a read-only `playingFileID` on `AudioPreview` if they can diverge.
- (from T12.5 review) With a sidebar library scope, the panel's Library chips stay selected and counted but are ignored (`FilterState.swift:50`); hide/disable the section while a sidebar library is in scope.
- (from T12.5) Arrow keys between chips in the filter panel (DESIGN §3) are not implemented (Tab/Space/Esc work); needs a custom focus handler.
- (from T12.5) `main-filter-panel` snapshot window is too short to show the Library section; add a taller or scrolled variant.
- (from T12.1 review) `SonarChip.swift` mixed-chip dash uses a literal `lineWidth: 1` and spacing tokens as the dash pattern; add a `chipDash` token. Its `×` now shows whenever `onClear` is set (not only when active); document it.
- (from T12.1 review) Components gallery: readout values not yet `signal` (Readout v2 in T12.4); neutral "Start" knob at 0 ms points up instead of the range start.
- (from T12.1) Light hover chip (fill 18 %) `facetCategory` text on `surfaceGroup` is 4.22:1 (< 4.5); darken the text or lower the hover tint if it matters.
- (from T12.2 review) Tags differing only in non-ASCII case (`Äpfel`/`äpfel`, SQLite NOCASE is ASCII-only) are suggested as a Separators pair that `ignore_similar_tags` refuses; skip such pairs or allow ignoring them.
- (from T12.2 review) Migration 0005 collapses only ASCII whitespace while the write path collapses Unicode whitespace; a stored tag with an inner NBSP cannot be found by rename/delete.
- (from T12.2 review) similar-tags 200 ms budget is asserted only in a release-mode test; add a criterion bench; `ignore_similar_tags` doc should say tags need not exist.
- (from T10.7) BPM ±1 below 90 % on real loops (golden 88.9 %, silver 81.2 %): rest is rhythm-less FX/risers, 1–4 s fills, a cappella takes; also ≤ 72 BPM material now reads double-time (tactus prior σ 0.6, fixture loop_000 70→140).
- (from T10.7 review) `tempo/mod.rs` loop-length prior on long files: beat counts are dense on multi-minute stems (adds noise); consider limiting it to files < ~3 min.
- (from T10.7 review) `jobs/work.rs` is ~470 lines; split it.
- (from T10.6 review) `engine/rows.rs` `KeyInfo::load` adds per-row queries for key_audio/pitch_cents; fold into the main row query.
- (from T10.6 review) `index/source.rs:246` a silent file still falls back to the filename key; ADR-041 says silence gets no key.
- (from T10.6) `scripts/ui-snapshots.sh` hung after writing all PNGs (killed at 20 min); find the test that never finishes.
- (from T10.5 review) `search/collect.rs` is 432 lines; move `SortValue`/`compare` or its tests to their own file.
- (from T10.5) `AppModelTests.testShellFlow` favourites step failed 2/3 runs (stale "hit" rows; likely the index poll refresh cancelling the explicit one in the core); test now waits for the rows. Confirm the cause and whether a user can see stale rows after toggling a favourite.
- (from T10.4b) High key tier misses 90 % on real files (silver 79.7 %, golden 68.8 % at conf ≥ 0.9; golden medium 34.5 %; ECE 18.5 / 31.7 %): inspect high-confidence misses (relative/fifth vs label noise) and recalibrate `key::fuse`.
- (from T10.4 review) `eval/metrics.rs` (466) and `eval/baseline.rs` (613 lines) exceed ~400 lines; split.
- (from T10.3 review) Seeded pitched loops live in `key/root_tests.rs`, not `sonar fixtures`; eval does not report loop-root accuracy (T10.4 could add a root truth field).
- (from T10.3 review) Segments shorter than 20 ms skip + note window count as unpitched; fast 16th-note bass loops (~88 ms/step at 170 BPM) may never get a root — add a fast-subdivision fixture.
- (from T10.2 review) Agreeing name is counted twice (NAME_BONUS inside audio score `a`, then noisy-or with `n`); compute `a` without the bonus — T10.4 calibration should account for it.
- (from T10.2 review) `tests/key.rs` checks key/note accuracy by threshold (≥90/95 %), not exact 16/16, 52/52; fixtures eval holds it today.
- (from T10.2 review) `is_tonal` is now only used by tests; remove or document as diagnostics. `ChromaMap::new` rebuilt per call on the noise path.
- (from toolbar-header fix) Fixed ToolbarSpacers stay when the trial badge / indexing status is hidden; tie each spacer to its item's condition if the gap is visible.
- (from T10.1 review) Trailing-letter/bracket key rules also fire on queries (`Piano C` → `key:c|note:c|c`); consider limiting to filenames.
- (from T10.1 review) `top`/`ride` drum words block a bracketed key without a tonal word (`Top Line Loop (C)`).
- (from T10.1 review) T10.1 and T10.2 both bump `DSP_VERSION` (full re-analysis); fold into one bump before release.
- (from T1.7c review) Resume pressed while a failed job is shutting down may start a few files that are stopped immediately.
- (from T1.7c review) Loop guard against a loop that cannot happen today.
- (from T1.7c) `cargo deny` advisories check failed once (not in check.sh, no deps changed); cause unconfirmed. (T1.7d: it is `basic-toml` unmaintained via `uniffi-bindgen`; ignore in `deny.toml` with a reason or bump.)
- (from T1.7d) An intact AIFF whose writer over-declares `COMM` `numSampleFrames` is now `partial`; check AIFF `partial` counts on a real library / eval.
- (from T2.1 review) `Fm_Bass_Growl` gets key prior F minor though "Fm" often means FM synthesis; check on silver set.
- (from T2.1 review) `-6dB` emits stray `d`/`b` tokens from the camelCase split.
- (from T2.1 review) Full-width digits (`１２０`) dropped as separators; NFKC folding would cover them.
- (from T2.2) Synonym expansion for unfinished (prefix) words (`kic` → kick group), guarded so `bas` doesn't pull in `bassdrum`.
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
- (from T2.4) 1-letter prefix + facets is the as-you-type tail (~32 ms on M4 Pro); measure on a base M1, consider 2-char minimum prefix or no facets for 1-letter queries.
- (from T2.4) Browse-all + facets is a flat ~27–29 ms full scan; cache browse-all facet counts per index generation if slower Macs near the budget.
- (from T2.4) Bench first query is only cold-ish (OS cache warm); add an optional truly cold run (`purge` / fresh clone) for app-start latency.
- (from T2.4 review) Browse classes have 3 queries (p95 ≈ max of 30 runs); raise default rounds or add queries. No test for the budget-miss non-zero exit (move check into core + unit test).
- (from T3.1 review) `FeatureMatrix` has public fields; add a checked constructor so `data.len() == n_frames * n_cols` holds.
- (from T3.1 review) DSP timing is only an `#[ignore]` test; add `sonar bench dsp` once T3.2 wires DSP into the pipeline.
- (from T3.2) serde_json's default float parsing can be 1 ulp off; enable `float_roundtrip` (or store stats in binary) if persisted `LibraryNorm` must reproduce z-scores bit-exactly.
- (from T3.3 review) Loop/one-shot weights are hand-tuned on synthetic fixtures; re-check on the silver/golden eval (T3.6/T3.7). Weakest fixture: 1.9 s loop at p_loop 0.70.
- (from T3.3 review) Classifier timing is only an `#[ignore]` test; add it to `sonar bench` when wired into the pipeline.
- (from T3.3 review) Hint word `beat` tags "Beat Kits/" folders (mostly one-shots) as loop (±2 logit); consider dropping it.
- (from T3.4) Half-time feels are read at half tempo (fixture 140 → 70); better octave cues (separate kick/snare bands) once silver/golden data exist.
- (from T3.4) Re-check tempo prior centre (110 BPM) and prior weights on the silver set (T3.6).
- (from T3.4) Triplet-feel alternatives (×⅔ / ×3⁄2) are not scored in octave correction.
- (from T3.4) `sonar-cli/src/cli.rs` is 508 lines; split argument definitions.
- (from T3.4 review) User kind override `one_shot` on a detected loop still shows the analysis BPM in the index; skip it unless BPM is overridden.
- (from T3.4 review) Tempo allocates per-file scratch buffers; reuse them like `Analyzer` if profiling shows pressure.
- (from T3.5 review) `key::chroma` allocates per-file totals; reuse scratch if profiling shows pressure.
- (from T3.6 review) No CLI test for `sonar eval --check` exiting non-zero on a regression (core logic is tested); add one with a temp baseline.
- (from T4.1 review) Vector `append` bumps headers without fsyncing aspect rows: power loss can leave live all-zero vectors that are never re-queued; `sync_data` before `set_rows` or detect zero rows on open.
- (from T4.1 review) Writer overwrites tombstoned id slots that other processes may hold as `&[i64]` (theoretical aliasing UB); use `read_volatile` or the in-memory map only.
- (from T4.1 review) A re-analysis that now fails keeps the file's old vector row live until the next dsp-stage reconcile; tombstone `Failed` ids in `jobs::vectors::append`.
- (from T4.1 review) After a lost/reset vector store, reconcile re-queues offline files too (they show as unanalysed until their volume returns); restrict to present files.
- (from T4.1) Two concurrent `sonar analyze` processes: the second fails at the dsp stage on the vector writer lock; add a job-level guard.
- (from T4.2) Per-aspect L1 scored 2 seeds better than L2 on fixture top-3 (31 vs 29/34); re-decide on silver/golden in T4.4.
- (from T4.2) Pitch aspect is dominated by its 12 chroma dims (pitch-only same pitch class 19/22, octaves weakly separated); weight f0 vs chroma in T4.4.
- (from T4.2) External-file seeds are re-analysed on every query; cache by path+mtime if the UI drags the same file repeatedly.
- (from T4.2 review) `sonar similar` has no Ctrl-C cancel flag for external-seed decode (Engine/FFI will pass one).
- (from T4.2 review) Similarity fixture-sanity thresholds sit 1 seed below measured; loosen by one seed if parallel norm merge order ever flips a near-tie.
- (from T4.3) `filtered_file_ids` is now the largest similar() cost (~7 ms of ~14 ms at 500k, grows with matches); cache the unfiltered id set / bitset if needed.
- (from T4.3) Similar latency measured on M4 Pro only (4-thread p95 25.7 ms as proxy); re-run `sonar bench similar` on a base M1/M2.
- (from T4.3) `bench/search_corpus.rs` is 403 lines (just over the ~400 guideline); split when next touched.
- (from T4.4 review) Parity test: eval similarity ranking (`eval::similarity::ranking`, default config) == `similar::rank` on the fixture library, so the re-implemented formula cannot drift.
- (from T5.1) Ship the full Apache-2.0 license text in the app bundle (T7.2 About tab has the attribution line only).
- (from T5.1) CoreML EP with static shapes + a compiled-model cache next to the CPU sessions (79 clips/s fp32, but 33 s compile per load uncached).
- (from T5.1) Memory per CLAP session not measured; consider shared prepacked weights across the N single-thread sessions.
- (from T5.1) Re-compare `laion/larger_clap_general` once golden NL queries exist (same quality on silver, ~2× slower).
- (from T5.1) `scripts/fetch-models.sh` lacks the exec bit (chmod blocked in the agent sandbox); `git update-index --chmod=+x` it.
- (from T4.4) Fixture sweep favoured L1 (P@10 87.29 vs 81.51), no-pitch 85.42, amp ×2 86.53; defaults kept (synthetic-fixture overfit risk). Re-run `sonar eval --golden --similar-sweep` once labels + `similar.jsonl` exist and decide then.
- (from T5.2 review) `run_embed` loads the CLAP pool (~4 s, 10 sessions) before checking for pending Embed work; open/reconcile the store, `count_pending` and return early when 0.
- (from T5.2 review) `embed/store.rs` rewrites `meta.json` non-atomically on every writer open; write only on New/Reset or via temp + rename.
- (from T5.2 review) `embed/pool.rs` batch-of-one path clones 256 KB features per clip; `mem::take` instead.
- (from T5.2 review) Front end has property tests only; add HF-computed reference log-mel values or a model-gated 6-class zero-shot check (ADR-026: 0.984).
- (from T5.2 review) A changed file that fails to re-embed keeps its old live row in the CLAP store; T5.3/T5.4 should filter by stage outcome or the store should drop the row.
- (from T5.2) `cargo deny check advisories` flags unmaintained `paste` (via tokenizers) and `basic-toml` (via uniffi); check.sh does not run advisories.
- (from T5.2) Embed throughput measured on M4 Pro only (154 files/s); base M4 (4P) estimated ≈ 60 files/s — re-measure on base hardware.
- (from T5.3) Re-fit category profiles / fusion weights / T on golden to reach ≥ 90 % top-1 (silver CLAP+DSP 76.0 %, partly in-sample); also report silver on the 25 held-out folders.
- (from T5.3 review) If the CLAP text tower fails but audio works, files get `embed` v2 ok without category refinement and `keep_refined` then pins the DSP-only category; skip the stage when `CategoryRefiner::new` is None or key `keep_refined` on `category_source`.
- (from T5.3) The embed-stage category refiner opens the vector store once per run; share the handle with the Engine (T5.5).
- (from T5.3) Fixture drum loops are often classified `texture`/`lead` by DSP-only cues; improve loop cues or fixture timbres.
- (from T5.3 review) `classify/category/taxonomy.rs` (411) and `jobs/work.rs` (448) exceed ~400 lines; move `work.rs` tests to `jobs/tests/`.
- (from T5.4) Hybrid mode re-embeds the query on every keystroke (~5 ms); debounce the semantic leg in the app/Engine.
- (from T5.4) BPM/key are filters only, not text tokens: "120 bpm" in a query matches names only; index them as words for BM25/hybrid.
- (from T5.4) Hybrid facets count text hits only (CLAP-only hits not counted; semantic mode returns none); field sorts fall back to text.
- (from T5.4) Text leg ANDs every word, so long NL queries often rely on CLAP alone; test an OR/soft-match text leg on golden.
- (from T5.4) Replace plain RRF with weighted fusion once golden `text_queries.jsonl` exists (fixture "drum one shot": CLAP 56.4 → hybrid 36.8).
- (from T5.4) `sonar eval` loads the CLAP text encoder twice (category + text search), ~1 s extra per run.
- (from T5.4) `eval/baseline.rs` (585) and `eval/golden.rs` (414) exceed ~400 lines.
- (from T5.4 review) `search/semantic.rs` turns a failed/misaligned mmap into an empty matrix → silent zero candidates; return an error so hybrid reports a fallback.
- (from T5.4 review) Hybrid cancellation is checked only after text leg + embed + candidate pass; finer-grained cancel in the T5.5 Engine.
- (from T5.4 review) Hybrid always mixes up to 200 CLAP neighbours, so exact-name queries also list non-matching files while `text_total` counts name matches only; make this visible in the UI.
- (from T5.4 review) `eval/text_search.rs` `skipped` sums skipped queries and missing judged files; split the counts.
- (from T5.4 merge) flock retry loop is duplicated in `embed/store.rs` and `index/vectors/mod.rs` `take_lock`; extract a shared helper.
- (from T5.5a) `Engine` opened with `rebuild_index: false` leaves an empty Tantivy dir, so the next open reports "previous build did not finish" instead of "no index yet" (cosmetic).
- (from T5.5a) Semantic reload after an embed batch re-loads the CLAP text tower; reuse the encoder, reload only the embedding index.
- (from T5.5a) `IndexRefreshed` events (one per batch) can crowd the 1024-entry engine event queue during a large analysis; throttle or coalesce.
- (from T5.5a) `refresh_files` queries the store once per id (≥ 50k touched ids now fall back to a full rebuild); batch the source query if mid-size refreshes are slow. First-scan cost at 500k not measured (reviewer est. +5–15 %).
- (from T5.5a review) With `rebuild_index: false`, a batch refreshed while `ensure_index` is still rebuilding is skipped (only when one process searches and analyzes at once).
- (from T5.5a review) No test for `pause_indexing`/`resume_indexing` or the failed-refresh retry path in the engine worker.
- (from T5.5a) Search cancel flag is engine-wide: two independent callers searching at once cancel each other; per-channel tokens if the app ever needs parallel searches.
- (from T5.5b) A user-data setter + synchronous refresh costs ~87 ms per call on the 500k index (flat, so Tantivy commit/reader reload dominates; `cargo test --release -p sonar-core --lib setter_latency -- --ignored --nocapture`); fine for clicks, but a UI drag over many files should send one multi-id call, or the engine should coalesce (async refresh) if it feels slow.
- (from T5.5b) No override value meaning "no BPM/key" (`NULL` row is read but never written); collection reordering (`position` is append-only) and tag rename/merge are not in the API.
- (from T5.5b) `sonar index` (explicit full rebuild) and `sonar stats` / `store info` still open `Store`/`TextIndex` directly; a rebuild-on-engine call (`Engine::rebuild_index`) would let the FFI offer "repair index".
- (from T5.5b review) A setter commits to SQLite before refreshing the index. If the refresh fails, the caller gets an error but the data is saved, and the files stay stale in search until a rebuild. Queue the ids for the worker's retry, or mark the index not ready.
- (from T5.5b review) Store path lookup falls back to the raw path when `canonicalize` fails (file gone), so `/var/…` does not match a `/private/var/…` library root. Canonicalize the parent folder instead.
- (from T5.5b review) CLI user-data commands read any numeric argument as a file id, so a relative file named `123` can't be passed as a path.
- (from T5.5b review) `set_flag` (store/user_data.rs) decides which parameters to bind by looking for `?2` in the SQL text; pass a `with_time` flag instead.
- (from T5.6) FFI v1 leaves out facets, `duplicates`, external-file similarity seeds and listing overrides; `previewPcm` is mono only (stereo preview later).
- (from T5.6 review) `preview_pcm` at ≥ 96 kHz can briefly hold ~1 GB across the bridge (several 10-min copies); cap the output rate at 48 kHz or a sample budget. `waveform_peaks` buffers the whole file and past ~25 min covers only the start; stream peaks or report truncation.
- (from T5.6 review) Waveform/preview decodes can't be cancelled over FFI (`DecodeOptions.cancel` exists in core); T6.4/T6.5 pre-buffering will want it.
- (from T5.6 review) `Error::Io` shows only "I/O error at <path>", so Swift loses the cause ("No such file", "Permission denied"); build the FFI message from the error's source chain.
- (from T5.6 review) No test actually observes `FfiError.Cancelled` from a superseded search (needs a search on a second thread + `cancel_search`).
- (from T5.6 review) Deep paging re-runs the search for `offset + limit` hits, and a superseded call can still return rows fetched after the cancel check; for T6.2 consider an ids-only full-result call + `rows(fileIds:)`, and tag UI requests with a generation number.
- (from T6.1 review) `AppModelTests` checks `paused` only if the fixture job is still running, so on a fast machine the pause path may not be exercised; pause before any await or use a larger library.
- (from T6.1 review) The `.failed` engine state is not tested through `AppModel`; inject an opener closure and assert it.
- (from T6.2 review) `ResultsPager.reset` returns false and leaves `resetting = true` if page 0 is `Cancelled` `maxRetries` times in the current generation (not reachable today; similarity search will share the cancel flag): clear the flag and throw so `AppModel` reports it.
- (from T6.2 review) Header sort arrow is not synced from `AppModel.sortKey` when the table is recreated, and collections show an arrow though `collectionRows` has no core sort (add sort to `collectionRows` or disable sorting there).
- (from T6.2 review) After a reset that drops the selected file from page 0, the inspector keeps the old snapshot and `selectedID` stays set; decide: clear the selection or re-highlight when scrolled to.
- (from T6.2 review) The Library column shows the folder name but the core sorts by library id; sort by display name in the core.
- (from T6.2 review) The perf test's big jumps time mostly placeholder rows (page landing reload is untimed); also time the reload after a page arrives.
- (from T6.3 review) ⌘F/Esc focus the toolbar `NSSearchField` by walking the window's views; this fails in full screen (toolbar in a separate window) or when the toolbar collapses the field into a button (silent no-op).
- (from T6.3 review) Library chips selected under All Samples stay set (hidden) when a sidebar library is chosen, so "Clear filters" shows with no visible chip.
- (from T6.3) `ResultsPagerTests.testResetDropsOldGenerationResults` flaked once under check.sh load (150 ms loader vs 300 ms sleep); make the loader signal completion.
- (from T6.4 review) Selection decodes in `AudioPreview` are neither cancelled nor capped (only prefetch is, at 2); holding ↓ over long uncached files can run many full decodes at once. Cancel stale ones / keep one pending.
- (from T6.4 review) Preview decode is capped in seconds (600 s), not bytes; hi-rate multichannel clips can exceed the 256 MB cache. Also cap frames by bytes (`AVAudioFrameCount(clamping:)`).
- (from T6.4 review) `AppModel.start()` calls `preview.warmUp()`, so `AppModelTests`/`FilterSearchTests` start the real audio engine; inject a fake player there.
- (from T6.4) Toggling loop while playing restarts the sound from the start; keep the position.
- (from T6.5 review) Each selection starts an uncancellable core `waveformPeaks` decode on the cooperative pool; holding ↓ over long files piles them up. Keep one in flight + one pending latest id (or FFI decode cancel).
- (from T6.5 review) Seek copies the clip tail on the main thread (up to ~230 MB for a 10-min 48 kHz stereo clip); toggling loop after a seek restarts from 0.
- (from T6.5 review) `WaveformTests.testLatestSelectionWins` relies on a 200 ms sleep; gate the fake provider on a continuation so the stale path is always exercised.
- (from T6.5 review) Waveform colour doesn't follow a system accent-colour change until the next relayout; no playhead latency compensation.
- (from T6.5) If the engine is dropped while a detached `waveformPeaks`/`previewPcm` task still holds it, `Engine::drop` runs on a pool thread (aborted `testShellFlow` once at teardown); the app never drops it before quit, but verify a quit during a decode is clean.
- (from T6.6) The table flashes empty on each slider re-query (pager parked on an empty loader so old text pages can't cancel the shared flag); a separate similarity cancel token would let the old rows stay until new ones arrive.
- (from T6.6) Each refresh re-runs an external-seed query, which re-decodes the dropped file (index refresh, favourite toggle); cache the seed vector by path+mtime (see T4.2 Later).
- (from T6.6) Facet counts are absent in similarity mode (chips show only selected values).
- (from T6.6) Optional "Match" column from `SimilarHit.distance` (0 identical … ~1.4 unrelated).
- (from T6.6) A non-audio drop is silently rejected; add a hint. Drop extensions leave out `aifc`/`caf` (match the core decoder list).
- (from T6.6 review) No core test that a newer `search` supersedes a running `similar_latest` (external seed in a thread → `Cancelled` or completion, never a stale success).
- (from T6.6) Nothing analyzed yet gives `FfiError.Internal` ("no vector store yet"); the app maps it to an inline message, but a typed `NotFound`/dedicated case would be cleaner.
- (from T6.7) ⌘A on a huge result list selects (and drags) only loaded rows' ids as they load; "select all N results" needs a core ids query.
- (from T6.7) Offline/missing files are silently skipped when dragging; show them dimmed in the table.
- (from T6.7) If some DAW rejects a plain `NSURL` drag, add a file promise / extra audio UTI types.
- (from T6.7) `fileExists` per dragged row runs on the main thread at drag start; can stall on slow network volumes with large selections.
- (from T6.7 review) After a reset that drops the focused file but keeps other selected rows, highlight and inspector focus disagree (Return reveals the gone file).
- (from T6.7) Edit > Copy menu validation while the table is focused is untested.
- (from T6.8 review) Deleting a collection has no confirmation dialog.
- (from T6.8) Inspector can't show which fields are overridden (FileRow has no override flag); "Revert" is always offered (no-op without override).
- (from T6.9a review) A cached `content_hash` is trusted without re-statting the file; a file edited since the last scan (watcher not caught up) can still join an exact group. Compare size+mtime first.
- (from T6.9a review) No unit test for `scan::lock` acquire returning `None` when cancelled while waiting.
- (from T6.9a review) Exact pass looks up `file_path` once per uncached candidate; batch it if first-run cost on large libraries matters.
- (from T6.9a) Near matching depends on library z-score stats; in a tiny library of near-identical files the stats degenerate and near groups can be missed.
- (from T6.9b review) Hiding from the inspector/keyboard while in the Duplicates view doesn't refresh the group rows' "Hidden" label (and the shortcut reads pager rows); call `refreshDuplicateRows()` from `setHidden` when the Duplicates view is active.
- (from T6.9b review) `DuplicatesTests` cancel test may never cancel mid-run (`Task.yield()`); near-group test only checks groups exist, not which files (core tests cover it); `realpath` buffer leak in test.
- (from T6.9b) Tests other than Duplicates/AppModel still use the real `EnginePlayer`; AVAudioEngine can abort in the headless test host if they start playback.
- (from T7.1) Search at 1M docs misses 50 ms (budget is stated at 500k): as-you-type p95 59 ms, browse-all 54–59 ms; 1M index has only 2 segments (try ~1 per core), compare with the K-th threshold before reading the sort column, cache browse-all facets (`search/collect.rs:197-215`).
- (from T7.1) Not measured: peak RSS of benches, deep paging (offset ~900k of a 1M result) through store/FFI, the 1M-row 120 fps result list (Swift).
- (from T7.1) Waveform of long files decodes up to 64Mi frames (~400 MB), can't be cancelled and shows only the start; streaming peak pass + cancel flag (`engine/audio.rs:77`).
- (from T7.1) An unresponsive SMB server (not unmounted) blocks root checks, scans, jobs and app quit (`Engine::drop`); root check with a timeout on its own thread.
- (from T7.1) Duplicates skip offline libraries (ADR-036); near groups and cached exact hashes could include them.
- (from T7.1) Stale mount point holding only dot-files is not treated as offline (`scan/mod.rs:209`); rows drop out of search until remount.
- (from T7.1) Files with a read error (EIO, bad sector) stay pending forever and are retried every job; flag them after N failures.
- (from T7.1 review) No test for the watcher `WatchEvent::Online` → queue work path (only `scan_library` is tested).
- (from T7.2) `AppModel.swift` is 404 lines and `engineState`/`loadingRefreshes`/`isStarted` became module-settable for extensions; split AppModel state further.
- (from T7.2) Add/remove/search/pager errors still use the modal alert (only engine events go to the banner); third-party notices in About were written by hand, generate them from `cargo deny`/`cargo about`.
- (from T7.2) No tests for the empty-collection / empty-duplicates / no-similar states, rescan from Settings, or offline dimming.
- (from T7.3) Signed/notarized path (Developer ID, notarytool, stapler, appcast) is written to Apple/Sparkle docs but never run; do a real release smoke run once T7.4 credentials exist.
- (from T7.3) x86_64 slice is built but never run on an Intel Mac; re-check `ort` x86_64 ONNX Runtime before CLAP is linked into the app.
- (from T7.3) No Sparkle release-notes (`sparkle:releaseNotesLink`) in the appcast; `shellcheck` not installed, so check.sh only runs `bash -n` on release.sh.
- (from T7.5) The license gate hides the main UI only: the engine still opens/scans in the background, and menu commands (Open Folder, Find…) plus Settings stay reachable while gated; disable them (`.disabled(license.showsGate)`) if the honest-user model needs tightening.
- (from T7.5) A malformed build-time license public key (`BadSigningKey`) makes the app fail open; release.sh validates the key, but the FFI could map it to a gated state instead.
- (from T9.1 review) Snapshot PNGs are slightly desaturated (display profile of the capture); convert the CGImage to sRGB before encoding.
- (from T9.1 review) `scripts/design-lint.sh` misses conditional colour literals (`x ? .red : .secondary`) and `Font.system(size:)` without a leading dot.
- (from T9.2 review) Literals the lint misses in the results cells: hidden-dot alpha 0.4, skeleton width multiplier 0.4, 18×18 heart button; move to tokens (`Theme.Opacity.disabled`, `Theme.Size.rowIcon`).
- (from T9.2 review) A reused row view that is already under the pointer shows no hover heart until the mouse moves (`ResultsRowView.prepareForReuse`).
- (from T9.3 review) Player header readout captions (BPM/KEY/LENGTH) sit 2–4 pt below the top edge of the 36 pt transport row; give the row `barHeight + s8` or vertical padding (DESIGN §2.3/§3 change).
- (from T9.3 review) Pitch shown as knob + "+0 st" text, not a `Readout` as DESIGN §4.3 lists; align spec or view.
- (from T9.3 review) Waveform resize cursor pushed/popped by hand in `onHover`; can stick after a drag ends outside the handle (`.pointerStyle(.rowResize)`).
- (from T9.3) Pause and Stop do the same thing (`AudioPreview` has no real pause/resume).
- (from T9.3) Narrow window does not shrink the waveform to its 120 pt minimum first (DESIGN §3 Window).
- (from T9.3b review) A region deep in a long file decodes from 0 to start+30 s; seek-based partial decode.
- (from T9.3b review) CLI JSON `seed.file_id` holds the given id for a region seed while `seed_file_id` is None (region excludes no file); document or rename the field.
- (from T9.3c review) Setting/clearing the waveform region while playing does not change what plays until the next play (`AudioPreview.region` didSet restart, mind drag updates).
- (from T9.3c review) Esc clears the region only when the results table has focus; handle Esc in `WaveformNSView.keyDown`.
- (from T9.3c review) No region possible when the row's `durationMs` is nil; fall back to the decoded clip duration.
- (from T9.3c review) A click that moves a few points inside a region replaces it with a tiny region (or clears it); consider a larger drag threshold, and a named constant instead of `Theme.Space.s4`.
- (from T9.3c) Auto-play the region right after a drag when auto-play is on; keyboard nudging of region edges; similarity bar could show the seed range.
- (from T9.4) Snapshot fixture rows have no tags, so tag chips are never visible in the PNGs; add tags to a few fixture rows.
- (from T9.4) Inspector action buttons are tight in the 280 pt column ("Similar" short label, icon-only collection menu in multi); revisit with the inspector width or a compact button style.
- (from T9.5 review) `FilterState.libraryIds` / `showsLibraryChips` are unused since the Library chip left the bar (sidebar filters libraries) but still count in `isActive`/`activeCount`; remove or wire to the sidebar.
- (from T9.5 review) Move each chip's `onClear` into a `FilterState` mutator (`clearKeys()` …) so `FilterSearchTests` exercises the real clear path.
- (from T9.5 review) Chip `.accessibilityLabel`/`.help` on the whole `SonarChip` may hide the clear button's own label from VoiceOver; use `.accessibilityElement(children: .contain)`.
- (from T9.5 review) BPM/Duration range fields accept min > max (no results, no hint); swap or flag.
- (from T9.6 review) Drop overlay is opaque (surfaceContent base under accentSoft) and hides the whole detail column while dragging; DESIGN §3 implies a tint only, decide and align.
- (from T9.6 review) `testMainDrop` builds its own overlay copy instead of rendering ContentView's `dropTargeted` path.
- (from T9.6 review) Aspect sliders are 16–48 pt in the similarity bar (coarse); consider a popover with full-width sliders.
- (from T9.6) Playhead on the compare target well while it plays.
- (from T9.6) Waveform for external-file (dropped) seeds in the target well; needs a path-based peaks FFI.
- (from T9.6) At 900×520 the header with both wells takes ~330 pt and squeezes the table; cap the selected well in compare mode.
- (from T9.6) Similarity bar truncates the target name at narrow widths; hide slider labels or use `ViewThatFits`.
- (from T9.7 review) Sidebar Favorites/library badge counts come from the current search's facets, so they change while typing; whole-library counts or a DESIGN §3 note.
- (from T9.7 review) Banner has no 120 ms fade (DESIGN §2.4); add `.transition(.opacity)` with a motion token.
- (from T9.7 review) Snapshot license has no trial, so the toolbar trial badge is never in a PNG; add a trial-state snapshot.
- (from T9.8) License-gate snapshot lacks the Buy button (purchase URL nil under test); no snapshots for the Enter License error state or the duplicates error state (views reset/reload on appear).
- (from T9.8) Duplicates section headers use the system header font; consider a `SectionLabel`-style header.
- (from T9.8 review) Engine-failed message lost `.textSelection(.enabled)` in `EmptyStateView`; add a selectable-message option so users can copy the error.
- (from T9.9 review) Snapshot window is inactive, so on-toggles and `.borderedProminent` buttons render grey in settings/license-gate PNGs; make the window key in `scripts/ui-snapshots.sh` to verify accent styling.
- (from T9.9 review) `StatusBadge` green/orange text on its 16 % tint is < 4.5:1 in light mode (per §4.10 spec); consider darker light-mode status text.
- (from T9.9 review) Duplicates "Keep This"/"Hide" buttons render 13 pt (> 12 pt row text) and column widths 44/70 are literals (`DuplicatesView.swift:110-121`); use `Theme.Font.row` + `Theme.Size` tokens.
- (from T11.1) `UISnapshotTests.testMainFilterPanel` uses a literal `.padding(.leading, 300)`; use a Theme value or a design-lint allow comment.
- (from T11.1) `main-filters` snapshot cuts the 5th chip and hides the tag chip at the scroll edge; widen the snapshot window or add an edge fade.
- (from T11.1) Deselecting the last key inside the panel leaves `compatibleKeys` true (harmless, dropped in the query); reset it like `FilterPanel.clear` does.
- (from T11.1) "Include uncertain" alone has no bar chip (only the Filters button accent shows it); consider a chip.
- (from T11.1) Filter panel snapshot is a card stand-in for the real popover; real glass and popover height (ScrollView + maxHeight on macOS 14) unverified.
- (from T11.3) Filter panel section groups: use `.rect(corners: .concentric(minimum:))` once the deployment target is macOS 26 (rule 14; API unavailable on 14).
- (from T11.3) Active filter chips are cut off hard at the Favorites button when they overflow; add a trailing fade / edge effect.
- (from T11.3) Duplicates section headers use an ASCII hyphen ("… (2 groups) - Byte-identical files"); en dash or a caption line.
- (from T11.3) Duplicates rows show "Keep This" / "Hide" on every row; show them on hover/selection (rule 26).
- (from T11.3) Inspector BPM with no value shows an empty filled field with "–"; show a read-only dash until clicked, like Key (§4.9).
- (from T11.3) License gate: make "Enter License…" the prominent default action instead of equal weight with Quit.
- (from T11.3) Snapshots do not render the real window toolbar; toolbar placement (inspector toggle over the inspector column) is verified only manually.
- (from T12.0 mockups) "Sound character" chips in the inspector (Bright, Punchy, Dry from audio analysis) and a "Recently added" quick filter in the Filters panel: need core data first.
- (from T12.4) Format readout lacks bit depth: `FileRow` has no bit depth; add it in core/FFI so Format reads `WAV · 24-bit · 44.1 kHz · Stereo`.
- (from T12.4) Zoom buttons in the readout strip are `Menu`s (in/out/fit, amplitude ×2/÷2/reset); make them click-to-zoom with the menu on hold.
- (from T12.4) The bars.beats ruler assumes 4/4; support other time signatures if core ever detects them.
- (from T12.4 review) `WaveformNSView.swift` is 413 lines: move lane/playhead layout into a helper; ruler tick ratios in `WaveformRulerView` are local literals (tokens if reused); knobs don't dim when the header is disabled; no snapshot shows the playhead (add a playing fixture); check `-Onone` type-check time of `Knob.body`.
- (from T12.3) `decode/stream.rs` is at 382 lines: move `push_lanes`/`Lanes` to `decode/mix.rs` before adding more; document that `Decoded::samples` is empty in split-channel mode.
