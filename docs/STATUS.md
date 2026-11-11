# Status

Living log maintained by the agents. Newest entries on top in "Log".

## Current
- Milestone: M4 (M0–M3 done, tags m0-done … m3-done; T3.7 human)
- Next task: T4.3
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
- 2026-11-11 — T4.2: `sonar_core::similar` + CLI `sonar similar <id | --file PATH> [--weights] [search filter flags]` (ADR-024): raw rows z-scored with LibraryNorm, per-aspect RMS z-distance, D = Σwₐdₐ/Σwₐ, brute force over the filtered id set (hidden excluded, seed never a hit); external seed via the dsp stage's own `window_aspects`. **Fixtures: same-family top-1 34/34, top-3 majority 29/34; pitch-only same pitch class 19/22; external copy found at D≈0.** Reviewer APPROVE round 1 (1 trivial NIT fixed). Fixture eval --check green, metrics unchanged (similarity P@10 is T4.4). Later: 5 T4.2 items.
- 2026-11-11 — T4.1: vector store `index::vectors` (ADR-023): per-aspect mmap f32 matrices in `<db>/index/vectors/gen-<n>/`, ids file → in-memory map (last row wins), crash-safe append (min header count), compaction to new generation + atomic `CURRENT`, `LibraryNorm` in `norm.json` (per-batch merge, rayon rebuild). dsp stage stores raw aspects + reconciles (`DSP_VERSION` 4→5). CLI `sonar vectors [--compact]`. **1M×135 f32 open 17 ms (budget 1 s), first full pass 87 ms.** Reviewer APPROVE round 1 (2 trivial NITs fixed). Fixtures eval --check green, metrics unchanged. Later: 5 T4.1 items.
- 2026-11-11 — T3.6: `sonar eval` (`sonar_core::eval`): `--fixtures` / `--silver <dir>` (labels from names) / `--golden` (labels.csv, `--init-golden` template); analysis runs the dsp stage's own `analyze_window` with names hidden. Metrics per EVAL.md (kind, BPM ±1 + octave errors, key exact/MIREX, note PC/MIDI, decode errors); category/similarity/NL listed as not yet available. JSON + md report in eval/reports; `--check` (in check.sh) fails on > 1 pt drop / missing metric / broken baseline; `--write-baseline`. **Baseline = fixtures: kind 100, BPM 97.06, key 100, note 100.** Reviewer APPROVE round 1 (3 trivial NITs fixed). Later: 1 T3.6 item.
