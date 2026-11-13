# Status

Living log maintained by the agents. Newest entries on top in "Log".

## Current
- Milestone: M6 (M0–M5 done, tags m0-done … m5-done; T3.7 human)
- Next task: T6.3
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

## Metrics baseline
_(Filled by eval runs; see eval/reports/BASELINE.md.)_
- Fixtures (2026-11-11, 120 files, names hidden): kind 100 %, BPM ±1 97.06 % (1 octave error / 34 loops), one-shots with BPM 0, key exact/MIREX 100 %, note PC/MIDI 100 %, unexpected decode errors 0. Similarity (2026-11-12, 75 seeds): P@10 81.51 %, MRR 99.33 %, pitch-only P@10 72.84 %. Category (2026-11-12, 56 labelled, audio only): DSP top-1 33.93 % / macro-F1 50.72 % (tracked), CLAP+DSP 53.6 %. Text/NL search (2026-11-12, 19 queries): NDCG@10 BM25 28.96 % (tracked), hybrid 91.28 %. Silver (T5.3, 7818 files): category CLAP+DSP 76.0 %. Golden: not run.

## Log
- 2026-11-13 — T6.2: Results table is an AppKit `NSTableView` (view-based reused cells, 22 pt rows, 7 columns) over `ResultsPager`, a lazy LRU page cache (≤ 64 pages × 200 rows, serialized fetches, generation reset, `Cancelled` retry). Header clicks re-query the core with `SortKey` + direction (no Swift sorting; collections don't sort yet). New search scrolls to top; refreshes keep position; selection kept by file id. **XCTest scroll benchmark, 1M synthetic rows in a real table, 1,901 steps: p50 1.8 / p95 3.1 / max 6.8 ms (bound 16), cache peak 64/64 pages.** 6 pager unit tests; `AppModelTests` checks name sort asc/desc. check.sh green. Reviewer APPROVE round 1 (1 trivial NIT fixed: page-load errors surface). Later: 5 T6.2 items.
- 2026-11-13 — T6.1: App shell. `NavigationSplitView` with a sidebar (All Samples, Favorites, Libraries with add via NSOpenPanel / ⌘⇧O and remove with confirmation, Collections), `.searchable` search, a SwiftUI `Table` of the first 500 rows (T6.2 replaces it with NSTableView), and an `.inspector`. Indexing progress is polled (250 ms while running) with Pause/Resume. An interrupted job resumes at launch. `@MainActor` `AppModel` + `CoreBridge` (every blocking FFI call is detached; latest search wins by generation number and drops `Cancelled`). **XCTest `AppModelTests` drives the model on the fixtures library: add → pause/resume → finished → search → favourites → collection → overlapping searches → remove.** check.sh green. Reviewer round 1: CHANGES (latest-wins test could not fail), fixed together with 4 NITs. Round 2: APPROVE. Later: 3 T6.1 items.
- 2026-11-13 — T5.6 (M5 complete): FFI v1 (ADR-032). `SonarEngine` UniFFI object over the Engine covers open, libraries, watching, indexing start/pause/resume/cancel with polled progress and events, paged `search` (display rows, `hasMore`, latest-wins `Cancelled`), `similar`, `row`/`rows`, every user-data setter, `waveformPeaks` and mono `previewPcm` (Ogg ok). Every export runs under `catch_unwind`, and errors map to 6 `FfiError` cases. New core `engine::{rows,audio}`, CLI `sonar wave`. **XCTest `EngineBridgeTests` drives a `sonar fixtures` library (add → index → search → favourite → search → peaks → Ogg PCM) in check.sh; check.sh fails the test if fixtures are missing, so it cannot skip.** check.sh green. Reviewer APPROVE round 1 (5 trivial NITs fixed). Later: 6 T5.6 items.