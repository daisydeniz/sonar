# Status

Living log maintained by the agents. Newest entries on top in "Log".

## Current
- Milestone: M5 (M0–M4 done, tags m0-done … m4-done; T3.7 human)
- Next task: T5.1
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

## Metrics baseline
_(Filled by eval runs; see eval/reports/BASELINE.md.)_
- Fixtures (2026-11-11, 120 files, names hidden): kind 100 %, BPM ±1 97.06 % (1 octave error / 34 loops), one-shots with BPM 0, key exact/MIREX 100 %, note PC/MIDI 100 %, unexpected decode errors 0. Similarity (2026-11-12, 75 seeds): P@10 81.51 %, MRR 99.33 %, pitch-only P@10 72.84 %. Silver/golden: not run (env vars unset).

## Log
- 2026-11-12 — T4.4: `sonar eval` similarity metrics (`eval::similarity`): same-family queries over fixtures, k = min(10, |relevant|); P@10 (tracked, ≥ 70) + MRR (tracked) + pitch-only same-pitch-class P@10 (report only); golden reads `similar.jsonl`; `--similar-sweep` reports weight/L1 variants. **Fixtures 120 files / 75 seeds: P@10 81.51 %, MRR 99.33 %, pitch-only P@10 72.84 %.** Sweep: L1 87.29, no-pitch 85.42, amp×2 86.53 → defaults kept (synthetic overfit risk). Silver/golden not run (env unset). Reviewer APPROVE round 1 (1 doc NIT fixed). Later: 2 T4.4 items.
- 2026-11-11 — T4.3: `similar::rank` scores 4096-id chunks in parallel (rayon, per-chunk top-K merged by (D, id), bit-identical to sequential); new `sonar bench similar` (500k text index + vector store, 240 end-to-end queries). **M4 Pro 14 thr, 500k: p95 89–91 → 13.9–16.9 ms (filtered 6.3; 4 thr worst 25.7); budget < 50 ms met.** USearch prototype (outside repo): recall@10 0.92–0.985 default weights, 0.12–0.22 pitch-only → ADR-025: exact brute force, no ANN, revisit at ~1.5M rows. Reviewer APPROVE round 1 (2 trivial NITs fixed). Eval --check green, metrics unchanged. Later: 3 T4.3 items.
- 2026-11-11 — T4.2: `sonar_core::similar` + CLI `sonar similar <id | --file PATH> [--weights] [search filter flags]` (ADR-024): raw rows z-scored with LibraryNorm, per-aspect RMS z-distance, D = Σwₐdₐ/Σwₐ, brute force over the filtered id set (hidden excluded, seed never a hit); external seed via the dsp stage's own `window_aspects`. **Fixtures: same-family top-1 34/34, top-3 majority 29/34; pitch-only same pitch class 19/22; external copy found at D≈0.** Reviewer APPROVE round 1 (1 trivial NIT fixed). Fixture eval --check green, metrics unchanged (similarity P@10 is T4.4). Later: 5 T4.2 items.