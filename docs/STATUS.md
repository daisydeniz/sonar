# Status

Living log maintained by the agents. Newest entries on top in "Log".

## Current
- Milestone: M0
- Next task: T0.2
- Loop state: RUNNING   <!-- RUNNING | BLOCKED | ROADMAP COMPLETE -->

## Toolchain (T0.1, 2026-11-09)
macOS 26.7.2 arm64 · Xcode 26.5 · xcodegen 2.46.0 · rustup 1.29.3 (Homebrew) ·
rustc/cargo 1.99.1 · cargo-deny 0.20.2 · claude 2.1.316 · gh: not installed (optional)

## Needs human
_(Agents list anything that requires the founder: decisions, credentials,
manual tests, labeled data. Keep each item one line with the task id.)_
- T0.1 (optional) For `cargo` in a normal terminal / hand-started `claude`, add to ~/.zprofile: `export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"` (scripts and autopilot already handle it). Optional: `brew install gh`.
- T3.7 Golden set labeling (when M3 starts)
- T8.1 End-to-end test by the founder (after M7)
- T7.4 Apple Developer ID, notarization, payment provider, pricing

## Metrics baseline
_(Filled by eval runs; see eval/reports/BASELINE.md.)_

## Log
- 2026-11-09 — T0.1: bootstrap.sh exits 0 (idempotent). rustup was missing → now installed via Homebrew; new `scripts/lib/env.sh` puts rustup/cargo on PATH for bootstrap, check and autopilot; version capture fails loudly. check.sh green (deny/eval/xcode stages not active yet).
- 2026-11-09 — Workflow: one branch per task, ff-merge to main, milestone tags; no founder approvals until M8.
- 2026-11-09 — Repository scaffolded (docs, agents, commands, Rust workspace stub).
