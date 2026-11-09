# Status

Living log maintained by the agents. Newest entries on top in "Log".

## Current
- Milestone: M1 (M0 done, tag m0-done)
- Next task: T1.1
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
- 2026-11-09 — T0.5: new crates `sonar-ffi` (UniFFI 0.32.2 proc-macro, staticlib+lib, exports `core_version()` → `sonar_core::version()`) and `uniffi-bindgen` (bindgen bin; `cli` feature isolated there). `scripts/build-core.sh` (debug by default, `SONAR_CORE_PROFILE=release` for release; arm64 only, MACOSX_DEPLOYMENT_TARGET=14.0) → gitignored `app/Generated/SonarCore.xcframework` (C module `SonarCoreFFI`) + `SonarCore.swift`; outputs replaced only on content change. Clean run 13.8 s, no-op 1.0 s. App: `ARCHS: arm64`, xcframework linked `embed: false`, `Services/CoreInfo.swift`, window shows "Core 0.1.0"; XCTest 6 (2 new, FFI call at runtime). check.sh green from scratch (rm Generated + xcodeproj); build-core now always runs. cargo deny ok (MPL-2.0 already allowed; extra duplicate-`syn` warn). Reviewer APPROVE; 3 nits fixed. Note for T1.x: fallible exports must return `Result<_, FfiError>` (non-throwing exports use `try!` in Swift → panic aborts app). Release pipeline (T7.3) must set `SONAR_CORE_PROFILE=release`. M0 complete.
- 2026-11-09 — T0.4: `app/project.yml` (XcodeGen) → SwiftUI app "Sonar" (`app.sonar.Sonar`, macOS 14, Swift 6 strict concurrency, hardened runtime ON, App Sandbox OFF, ad-hoc signing) + `SonarTests` (XCTest, hosted; 4 tests) + empty AppIcon/AccentColor asset catalog. Placeholder window shows name + bundle version. check.sh now runs xcodegen, xcodebuild build and test: all green, no compiler warnings. Reviewer APPROVE; fallback-test nit fixed. Reviewer notes for T0.5: build the core as a **staticlib** xcframework (with ad-hoc signing and hardened runtime, a dylib can fail to load with "no Team ID"); set `ARCHS: arm64` until the universal build exists; keep the cargo build in check.sh, not in an Xcode script phase (user-script sandboxing is on); put bindings under gitignored `app/Generated/`.
- 2026-11-09 — T0.3: `sonar fixtures <dir> [--seed N ≤ 2^53−1] [--skip-long]` → 115 files (tonal 22, formats 20 incl. all 18 depth×rate×channel combos + 2 extensible, loops 17 at 70–175 BPM incl. 122.5 and a 3/4 click loop, chords 16 keys (8 maj/8 min), corrupt 15 (11 `error`, 4 `error_or_partial`), oneshots 12, edge 9 (5 ms, 15 ms, 310 s, 0/1-frame, −60 dBFS, clipped), noise 4) + `fixtures.json` with a `fields` legend; ~104 MB, 4.8 s in a debug build. Hand-written WAV writer + SplitMix64 (byte-identical on the same platform); filenames neutral, truth only in manifest; schema in EVAL.md "Fixture manifest". Deps: serde, serde_json; dev: hound, tempfile. 72 tests (tests add ~12 s). Reviewer APPROVE; nits fixed.
- 2026-11-09 — T0.2: workspace deps anyhow 1.0.104, clap 4.6.7 (derive), thiserror 2.0.21, tracing 0.1.44, tracing-subscriber 0.3.23; `core/deny.toml` (permissive allow list, crates.io only; ADR-004 amended for Unicode-3.0 + LLVM-exception). CLI now clap-based (`sonar version`), core `Error`/`Result` via thiserror; 13 tests. check.sh green; deny runs `licenses bans sources` (advisories skipped: needs network). Known warn: duplicate `syn` 2/3 (tracing-attributes).
- 2026-11-09 — T0.1: bootstrap.sh exits 0 (idempotent). rustup was missing → now installed via Homebrew; new `scripts/lib/env.sh` puts rustup/cargo on PATH for bootstrap, check and autopilot; version capture fails loudly. check.sh green (deny/eval/xcode stages not active yet).
- 2026-11-09 — Workflow: one branch per task, ff-merge to main, milestone tags; no founder approvals until M8.
- 2026-11-09 — Repository scaffolded (docs, agents, commands, Rust workspace stub).
