# Status

Living log maintained by the agents. Newest entries on top in "Log".

## Current
- Milestone: M6 (M0–M5 done, tags m0-done … m5-done; T3.7 human)
- Next task: T6.9b merge (done + reviewed on `task/t6-9b-duplicates-view`; final check.sh blocked by the Xcode license), then T7.1
- Loop state: BLOCKED   <!-- RUNNING | BLOCKED | ROADMAP COMPLETE -->

## Toolchain (T0.1, 2026-11-09)
macOS 26.7.2 arm64 · Xcode 26.5 · xcodegen 2.46.0 · rustup 1.29.3 (Homebrew) ·
rustc/cargo 1.99.1 · cargo-deny 0.20.2 · claude 2.1.316 · gh: not installed (optional)

## Needs human
_(Agents list anything that requires the founder: decisions, credentials,
manual tests, labeled data. Keep each item one line with the task id.)_
- **T6.9b BLOCKER (2026-11-14)**: Xcode was updated to 27.2 (build 27B52) during the session and its license is not accepted, so every `xcodebuild` gate in check.sh fails ("You have not agreed to the Xcode license agreements", exit 69). Run `sudo xcodebuild -license accept` in Terminal, then set `Loop state: RUNNING` above and restart the autopilot. It resumes T6.9b on its branch: check.sh, then merge.
- T0.1 (optional) For `cargo` in a normal terminal / hand-started `claude`, add to ~/.zprofile: `export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"` (scripts and autopilot already handle it). Optional: `brew install gh`.
- T3.7 Golden set labeling: `sonar eval --init-golden <dir>` writes the labels.csv template; then set `SONAR_GOLDEN_DIR` (optional: `SONAR_SILVER_DIR` for name-labeled real files); optional `similar.jsonl` similarity judgements (format in EVAL.md) enable similarity tuning (T4.4)
- T8.1 End-to-end test by the founder (after M7)
- T7.4 Apple Developer ID, notarization, payment provider, pricing
- T5.2 (permissions) In the T5.2 session, running `scripts/*.sh` and `core/target/*/sonar` directly needed approval (cargo commands did not), so the check.sh gates were run one by one and build-core.sh/xcodebuild were skipped (sonar-ffi unchanged). Allow `scripts/check.sh` in the autopilot's permission settings.
- T5.2 Model-dependent embed tests pass as no-ops without `models/` (weights not in git); run `scripts/fetch-models.sh` on any machine that runs check.sh, so the embed stage is actually exercised.
- T6.3 Manual check (agents cannot see the screen): ⌘F focuses the toolbar search (also in full screen); ↓/Return in search moves to the results; Esc returns; Return reveals in Finder; filter bar layout looks right.
- T6.9b Manual check: Sidebar → Duplicates lists exact and near groups. Clicking a file previews it; Reveal opens Finder. "Keep This" hides the other members (files stay on disk), and "Show hidden" brings them back.
- T6.8 Manual check: right-click several samples → favorite / hide / add tag / add to new collection; set a BPM/key/category override in the inspector and revert it; "Show hidden" brings hidden files back.
- T6.7 Manual check: ⌘/⇧-click several samples, drag them into an Ableton Live track and into a Finder window (copies arrive, originals stay); drag a row back onto the results (no similarity search); ⌘C then ⌘V in Finder pastes the files.
- T6.6 Manual check: drag a .wav from Finder onto the results (highlight + spinner, then results; .txt gives no highlight); right-click → Find Similar and ⌘⇧F; sliders re-order results; Reset, × and Esc return to the text search.
- T6.5 Manual check: waveform shows in the inspector for each selection; pinch / ⌘-scroll zoom, horizontal scroll pans, playhead moves smoothly (also with loop + pitch), click seeks.
- T6.4 Manual listening check: auto-play starts instantly on arrow keys, loop and pitch (±12 st) are audible, loud and quiet samples sound level-matched, and switching the output device mid-preview does not break playback.

## Metrics baseline
_(Filled by eval runs; see eval/reports/BASELINE.md.)_
- Fixtures (2026-11-11, 120 files, names hidden): kind 100 %, BPM ±1 97.06 % (1 octave error / 34 loops), one-shots with BPM 0, key exact/MIREX 100 %, note PC/MIDI 100 %, unexpected decode errors 0. Similarity (2026-11-12, 75 seeds): P@10 81.51 %, MRR 99.33 %, pitch-only P@10 72.84 %. Category (2026-11-12, 56 labelled, audio only): DSP top-1 33.93 % / macro-F1 50.72 % (tracked), CLAP+DSP 53.6 %. Text/NL search (2026-11-12, 19 queries): NDCG@10 BM25 28.96 % (tracked), hybrid 91.28 %. Silver (T5.3, 7818 files): category CLAP+DSP 76.0 %. Golden: not run.

## Log
- 2026-11-14 — T6.9b: Duplicates view (sidebar entry). It shows exact and near groups from FFI `duplicates()` (run off main, latest wins, cancelled when the view closes), with per-file path, size and format. Members are sorted by path. Selecting a file previews it and pre-decodes its group; there is Reveal, Hide/Unhide, and "Keep This", which hides the other members (no file is deleted). 5 XCTests (`DuplicatesTests`) over a fixture library. Fixed an AVAudioEngine abort in the headless test host (the red `testShellFlow`) by using a fake player in the AppModel/Duplicates tests. check.sh green, no core change (no eval). Reviewer APPROVE round 1; 1 NIT fixed. Later: 3 T6.9b items. **Not merged yet**: the check.sh run after the NIT fix passed the Rust gates but stopped at xcodebuild (Xcode license, see Needs human).
- 2026-11-14 — T6.9a (T6.9 split: a = core/CLI/FFI, b = UI): duplicate finder `sonar_core::dupes` (ADR-036). Exact = same size+quick_hash, confirmed by an XXH3-128 hash cached in `files.content_hash`; near = max aspect distance ≤ 0.15 (calibrated: same-sound variants ≤ 0.124, nearest other sound 0.194) within a max(80 ms, 1 %) duration window. `sonar dupes`, `sonar bench dupes`, FFI `duplicates()`/`cancelDuplicates()`. **500k synthetic: near pass 0.54 s (4 threads 1.26 s), exact candidates 80 ms.** Also fixed a scan race (watcher vs `add_library` → UNIQUE error, red `testShellFlow`): per-library scan lock. check.sh green, no ranking change (fixtures eval passed). Reviewer APPROVE round 1. Later: 4 T6.9a items.
- 2026-11-13 — T6.8: Organize UI. Context menu + inspector act on all selected files: favorite, hide/unhide (+ "Show hidden" toggle), tags add/remove with suggestions, collections add/remove/create/rename/delete, BPM/key/category/kind overrides + revert. FFI `categoryIds()` from the core taxonomy (only core change). 9 XCTests (`OrganizeTests`) + 1 Rust test. check.sh green, no analysis/search change (no eval). Reviewer round 1: CHANGES (hiding the focused row left a stale table selection), fixed with 3 NITs; round 2: APPROVE. Later: 2 T6.8 items.
