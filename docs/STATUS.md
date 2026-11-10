# Status

Living log maintained by the agents. Newest entries on top in "Log".

## Current
- Milestone: M2 (M0, M1 done, tags m0-done, m1-done)
- Next task: T2.2
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
- 2026-11-10 — T2.1: `sonar_core::text` tokenizer + filename priors (ADR-016), CLI `sonar tokenize [--query] [--json]`. Splits separators/camelCase/digits, folds accents (`unicode-normalization`, MIT/Apache), canonical `120bpm`, `key:c#m`/`note:c#` (sharps), configurable pack-noise stop list; query mode also maps lowercase `am`/`dbm`/`eb` to key tokens. BPM/key priors conservative (bare number only with loop word/key, single letter only after BPM). ~170 table rows; ~0.72 µs/path release. Reviewer APPROVE (round 1 blocker: lowercase key queries — fixed). Later: 5 T2.1 items (one-shot note letter, Fm, lowercase priors, -6dB, full-width digits). No eval (T3.6).
- 2026-11-10 — T1.7d: exact PCM length (ADR-015 addendum). WAV/AIFF PCM counted by packet bytes (truncated WAV → exact surviving frames + `partial`; was up to +1,151 frames); AIFF `COMM` is the declared length (longer `SSND` capped, shorter → `partial`); truncated AIFF (was an error) read via a bounded chunk-walk header overlay → exact frames + `partial`. meta/dsp stage versions 1 → 2. Cost: meta 3k b128 warm 55,819 / 56,244 files/s vs 56,233 before (noise), cold `--clone` 57,692 / 55,195. Core lib 331 passed + 1 ignored. Reviewer APPROVE (round 1: ADR wording blocker fixed). Later: AIFF over-declared COMM → partial watch, `basic-toml` advisory. M1 complete (m1-done). No eval (T3.6).
- 2026-11-10 — T1.7c: measured first, then pipelined. `sonar bench analyze` now reports `StageTiming` (pool utilization = busy worker-s / (threads × wall), coordinator claim/commit/wait time, slowest file per batch; text + `--json`). Before (strict claim → compute → commit): pool idle 36 % (meta) / 14 % (dsp) of the wall at batch 128 (commit alone 23 % / 7 %). Change (ADR-013 addendum): new `jobs/batches.rs` keeps 2 batches in flight (`LOOKAHEAD`); the coordinator claims ahead and commits in claim order while workers compute; one IMMEDIATE tx per batch, forward-only id cursor (never rewound), stale-claim check + BUSY retry unchanged, claim-ahead only within the current stage. Pause/cancel: workers stop starting files instead of blocking; finished results are committed, unstarted claims are re-dispatched from a retry list after resume (dropped on cancel); all-unstarted batches skip the commit; on a claim/commit error in-flight batches are cancelled and drained. **Release, M4 Pro, 14 threads, files/s before → after (utilization):** meta 3k b32 37,783 → 48,631 (47 → 62 %), b128 40,565 → 56,233 (54 → 71 %), b512 49,409 → 60,910; meta 20k b128 36,501 → 43,219 (64 → 79 %), b512 40,704 → 45,860; meta 20k cold `--clone` b128 37,009 → 42,755; dsp 3k b32 6,874 → 7,845, b128 7,932 → 8,026 (within noise), b512 8,059 → 8,507; dsp 20k b128 6,820 → 7,975 (+17 %, 86 → 90 %), b512 7,127 → 8,461. Single runs (±5 %); reviewer re-run meta 3k b128 56,448 (73 %); after review fixes (3k, batch 128) 58,212 meta / 8,482 dsp. Default batch stays 128. Tests: core lib 320 passed + 1 ignored; new `pause_mid_batch_commits_finished_files_and_resume_redoes_none`, `pause_mid_batch_with_skipped_file_counts_every_file_once` (2 threads, batch 8, both workers parked deterministically, missing file skipped mid-batch, unstarted files below it retried after resume: done == total, skipped counted once; fails with `(16, 17, 2, 15)` against a cursor-rewind variant); kill -9 resume AC test unchanged and green. Reviewer: round 1 blocker (cursor rewind re-offered committed skipped files → double counting) + STATUS numbers; round 2 blocker (regression test did not reach the bug scenario) — fixed; nits fixed (error path cancels + drains in-flight batches via `Shared::abort` (not reported as a user cancel), empty commits on pause skipped, pool panic handler, stale docs, per-file mutex → AtomicBool). No eval (T3.6).
