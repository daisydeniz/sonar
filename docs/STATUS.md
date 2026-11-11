# Status

Living log maintained by the agents. Newest entries on top in "Log".

## Current
- Milestone: M4 (M0–M3 done, tags m0-done … m3-done; T3.7 human)
- Next task: T4.4
- Loop state: RUNNING   <!-- RUNNING | BLOCKED | ROADMAP COMPLETE -->

## Toolchain (T0.1, 2026-11-09)
macOS 26.7.2 arm64 · Xcode 26.5 · xcodegen 2.46.0 · rustup 1.29.3 (Homebrew) ·
rustc/cargo 1.99.1 · cargo-deny 0.20.2 · claude 2.1.316 · gh: not installed (optional)

## Needs human
_(Agents list anything that requires the founder: decisions, credentials,
manual tests, labeled data. Keep each item one line with the task id.)_
- T0.1 (optional) For `cargo` in a normal terminal / hand-started `claude`, add to ~/.zprofile: `export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"` (scripts and autopilot already handle it). Optional: `brew install gh`.
- T3.7 Golden set labeling: `sonar eval --init-golden <dir>` writes the labels.csv template; then set `SONAR_GOLDEN_DIR` (optional: `SONAR_SILVER_DIR` for name-labeled real files)
- T8.1 End-to-end test by the founder (after M7)
- T7.4 Apple Developer ID, notarization, payment provider, pricing

## Metrics baseline
_(Filled by eval runs; see eval/reports/BASELINE.md.)_
- Fixtures (2026-11-11, 120 files, names hidden): kind 100 %, BPM ±1 97.06 % (1 octave error / 34 loops), one-shots with BPM 0, key exact/MIREX 100 %, note PC/MIDI 100 %, unexpected decode errors 0. Silver/golden: not run (env vars unset).

## Log
- 2026-11-11 — T4.3: `similar::rank` scores 4096-id chunks in parallel (rayon, per-chunk top-K merged by (D, id), bit-identical to sequential); new `sonar bench similar` (500k text index + vector store, 240 end-to-end queries). **M4 Pro 14 thr, 500k: p95 89–91 → 13.9–16.9 ms (filtered 6.3; 4 thr worst 25.7); budget < 50 ms met.** USearch prototype (outside repo): recall@10 0.92–0.985 default weights, 0.12–0.22 pitch-only → ADR-025: exact brute force, no ANN, revisit at ~1.5M rows. Reviewer APPROVE round 1 (2 trivial NITs fixed). Eval --check green, metrics unchanged. Later: 3 T4.3 items.
- 2026-11-11 — T4.2: `sonar_core::similar` + CLI `sonar similar <id | --file PATH> [--weights] [search filter flags]` (ADR-024): raw rows z-scored with LibraryNorm, per-aspect RMS z-distance, D = Σwₐdₐ/Σwₐ, brute force over the filtered id set (hidden excluded, seed never a hit); external seed via the dsp stage's own `window_aspects`. **Fixtures: same-family top-1 34/34, top-3 majority 29/34; pitch-only same pitch class 19/22; external copy found at D≈0.** Reviewer APPROVE round 1 (1 trivial NIT fixed). Fixture eval --check green, metrics unchanged (similarity P@10 is T4.4). Later: 5 T4.2 items.
- 2026-11-11 — T4.1: vector store `index::vectors` (ADR-023): per-aspect mmap f32 matrices in `<db>/index/vectors/gen-<n>/`, ids file → in-memory map (last row wins), crash-safe append (min header count), compaction to new generation + atomic `CURRENT`, `LibraryNorm` in `norm.json` (per-batch merge, rayon rebuild). dsp stage stores raw aspects + reconciles (`DSP_VERSION` 4→5). CLI `sonar vectors [--compact]`. **1M×135 f32 open 17 ms (budget 1 s), first full pass 87 ms.** Reviewer APPROVE round 1 (2 trivial NITs fixed). Fixtures eval --check green, metrics unchanged. Later: 5 T4.1 items.
