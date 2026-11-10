# Status

Living log maintained by the agents. Newest entries on top in "Log".

## Current
- Milestone: M2 (M0, M1 done, tags m0-done, m1-done)
- Next task: T2.3
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
- 2026-11-10 — T2.2: Tantivy 0.27 text index (MIT, mmap + lz4 only; ADR-017) over the T2.1 chain (+ plural folding, `ANALYZER_VERSION` 2): fields name/path/tags/category/kind (user overrides win, missing files excluded). Query: AND of words, `-word`/`-"phrase"` exclusions (synonym-expanded), dis-max over alternatives + synonyms, prefix on the last word unless trailing space (no synonym expansion for prefixes). Synonyms `docs/synonyms.tsv` built in, overridden by `$SONAR_HOME/synonyms.tsv` or `--synonyms`. CLI `sonar search [opts] <query…>` (options before query words), `sonar index`; search builds on first use, scan refreshes. Core lib 377 passed + 1 ignored; CLI e2e scan→search→rescan. Reviewer APPROVE round 1 (0 blockers). Later: 8 T2.2 items (incremental refresh, watcher, cancel, prefix synonyms, …). No eval (no text-query set until T3.6/T5.4).
- 2026-11-10 — T2.1: `sonar_core::text` tokenizer + filename priors (ADR-016), CLI `sonar tokenize [--query] [--json]`. Splits separators/camelCase/digits, folds accents (`unicode-normalization`, MIT/Apache), canonical `120bpm`, `key:c#m`/`note:c#` (sharps), configurable pack-noise stop list; query mode also maps lowercase `am`/`dbm`/`eb` to key tokens. BPM/key priors conservative (bare number only with loop word/key, single letter only after BPM). ~170 table rows; ~0.72 µs/path release. Reviewer APPROVE (round 1 blocker: lowercase key queries — fixed). Later: 5 T2.1 items (one-shot note letter, Fm, lowercase priors, -6dB, full-width digits). No eval (T3.6).
- 2026-11-10 — T1.7d: exact PCM length (ADR-015 addendum). WAV/AIFF PCM counted by packet bytes (truncated WAV → exact surviving frames + `partial`; was up to +1,151 frames); AIFF `COMM` is the declared length (longer `SSND` capped, shorter → `partial`); truncated AIFF (was an error) read via a bounded chunk-walk header overlay → exact frames + `partial`. meta/dsp stage versions 1 → 2. Cost: meta 3k b128 warm 55,819 / 56,244 files/s vs 56,233 before (noise), cold `--clone` 57,692 / 55,195. Core lib 331 passed + 1 ignored. Reviewer APPROVE (round 1: ADR wording blocker fixed). Later: AIFF over-declared COMM → partial watch, `basic-toml` advisory. M1 complete (m1-done). No eval (T3.6).
