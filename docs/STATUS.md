# Status

Living log maintained by the agents. Newest entries on top in "Log".

## Current
- Milestone: M6 (M0–M5 done, tags m0-done … m5-done; T3.7 human)
- Next task: T6.8
- Loop state: RUNNING   <!-- RUNNING | BLOCKED | ROADMAP COMPLETE -->

## Toolchain (T0.1, 2026-11-09)
macOS 26.7.2 arm64 · Xcode 26.5 · xcodegen 2.46.0 · rustup 1.29.3 (Homebrew) ·
rustc/cargo 1.99.1 · cargo-deny 0.20.2 · claude 2.1.316 · gh: not installed (optional)

## Needs human
_(Agents list anything that requires the founder: decisions, credentials,
manual tests, labeled data. Keep each item one line with the task id.)_
- T0.1 (optional) For `cargo` in a normal terminal / hand-started `claude`, add to ~/.zprofile: `export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"` (scripts and autopilot already handle it). Optional: `brew install gh`.
- T3.7 Golden set labeling: `sonar eval --init-golden <dir>` writes the labels.csv template; then set `SONAR_GOLDEN_DIR` (optional: `SONAR_SILVER_DIR` for name-labeled real files); optional `similar.jsonl` similarity judgements (format in EVAL.md) enable similarity tuning (T4.4)
- T8.1 End-to-end test by the founder (after M7)
- T7.4 Apple Developer ID, notarization, payment provider, pricing
- T5.2 (permissions) In the T5.2 session, running `scripts/*.sh` and `core/target/*/sonar` directly needed approval (cargo commands did not), so the check.sh gates were run one by one and build-core.sh/xcodebuild were skipped (sonar-ffi unchanged). Allow `scripts/check.sh` in the autopilot's permission settings.
- T5.2 Model-dependent embed tests pass as no-ops without `models/` (weights not in git); run `scripts/fetch-models.sh` on any machine that runs check.sh, so the embed stage is actually exercised.
- T6.3 Manual check (agents cannot see the screen): ⌘F focuses the toolbar search (also in full screen); ↓/Return in search moves to the results; Esc returns; Return reveals in Finder; filter bar layout looks right.
- T6.7 Manual check: ⌘/⇧-click several samples, drag them into an Ableton Live track and into a Finder window (copies arrive, originals stay); drag a row back onto the results (no similarity search); ⌘C then ⌘V in Finder pastes the files.
- T6.6 Manual check: drag a .wav from Finder onto the results (highlight + spinner, then results; .txt gives no highlight); right-click → Find Similar and ⌘⇧F; sliders re-order results; Reset, × and Esc return to the text search.
- T6.5 Manual check: waveform shows in the inspector for each selection; pinch / ⌘-scroll zoom, horizontal scroll pans, playhead moves smoothly (also with loop + pitch), click seeks.
- T6.4 Manual listening check: auto-play starts instantly on arrow keys, loop and pitch (±12 st) are audible, loud and quiet samples sound level-matched, and switching the output device mid-preview does not break playback.

## Metrics baseline
_(Filled by eval runs; see eval/reports/BASELINE.md.)_
- Fixtures (2026-11-11, 120 files, names hidden): kind 100 %, BPM ±1 97.06 % (1 octave error / 34 loops), one-shots with BPM 0, key exact/MIREX 100 %, note PC/MIDI 100 %, unexpected decode errors 0. Similarity (2026-11-12, 75 seeds): P@10 81.51 %, MRR 99.33 %, pitch-only P@10 72.84 %. Category (2026-11-12, 56 labelled, audio only): DSP top-1 33.93 % / macro-F1 50.72 % (tracked), CLAP+DSP 53.6 %. Text/NL search (2026-11-12, 19 queries): NDCG@10 BM25 28.96 % (tracked), hybrid 91.28 %. Silver (T5.3, 7818 files): category CLAP+DSP 76.0 %. Golden: not run.

## Log
- 2026-11-13 — T6.7: Drag & drop out + multi-select. Results table allows ⌘/⇧-click, ⇧↑↓, ⌘A (keeps the focused row); selection kept by file id across resets and exposed as `AppModel.selectedFileIDs` (for T6.8). Inspector/preview/waveform/Find Similar follow the focused row. Dragging writes one file URL per loaded, existing row (copy/generic only, never move); a drag back onto the results doesn't start a similarity search. Return reveals all selected; ⌘C copies file URLs. 7 new XCTests (`DragOutTests`). check.sh green, no core change (no eval). Reviewer round 1: CHANGES (⌘A moved focus), fixed with 3 NITs; round 2: APPROVE. Later: 6 T6.7 items.
- 2026-11-13 — T6.6: Similarity UI. FFI `similarTo(SimilarQuery)` (ADR-035): seed is a library file or an external path, with aspect weights via core `aspectWeightsFromSliders` (weight = overall + slider), search filters, and the shared search cancel flag (latest wins, also during external decode). App: "Similar to" bar with Overall/Spectrum/Timbre/Pitch/Amplitude sliders (150 ms debounce), Reset and ×/Esc back to the text search. Find Similar from the context menu, inspector or ⌘⇧F. Dropping a Finder audio file on the results starts an external-seed search. Errors show inline. 3 Rust + 15 XCTests. check.sh green, ranking untouched (fixtures eval unchanged). Reviewer round 1: CHANGES (the old text loader could cancel the query → stuck spinner), fixed with 4 NITs; round 2: APPROVE. Later: 7 T6.6 items.
- 2026-11-13 — T6.5: Waveform in the inspector. One 8192-bucket `waveformPeaks` fetch per file (off main, 32-file LRU, latest-wins), re-bucketed per viewport in Swift and drawn as a `CAShapeLayer`. Zoom: pinch / ⌘-scroll / buttons (≤ 64×), pan by horizontal scroll. Playhead layer from `AudioPreview.playbackFraction()` (loop-aware), driven by a `CADisplayLink` only while playing. Click seeks. 14 new XCTests. Also fixed a flaky core test (a random temp dir name was read as a folder key prior). check.sh green, Rust analysis untouched (no eval). Reviewer APPROVE round 1 (3 trivial NITs fixed). Later: 5 T6.5 items.
