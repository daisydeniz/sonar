# Status

Living log maintained by the agents. Newest entries on top in "Log".

## Current
- Milestone: M5 (M0–M4 done, tags m0-done … m4-done; T3.7 human)
- Next task: T5.4
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
- Fixtures (2026-11-11, 120 files, names hidden): kind 100 %, BPM ±1 97.06 % (1 octave error / 34 loops), one-shots with BPM 0, key exact/MIREX 100 %, note PC/MIDI 100 %, unexpected decode errors 0. Similarity (2026-11-12, 75 seeds): P@10 81.51 %, MRR 99.33 %, pitch-only P@10 72.84 %. Category (2026-11-12, 56 labelled, audio only): DSP top-1 33.93 % / macro-F1 50.72 % (tracked), CLAP+DSP 53.6 %. Silver (T5.3, 7818 files): category CLAP+DSP 76.0 %. Golden: not run.

## Log
- 2026-11-12 — T5.3: `classify::category` (ADR-028): 17-class taxonomy (`docs/categories.md`); late fusion of CLAP best-prompt zero-shot + Gaussian naive-Bayes DSP cues, temperature-calibrated, +3 filename-word hint after calibration. `dsp` stage writes DSP+name category, `embed` stage refines with CLAP. CLI `sonar classify`, `sonar eval --no-clap`. **Audio-only (names hidden): fixtures DSP 33.9 % (F1 50.7, tracked) / CLAP+DSP 53.6 %; silver 7818 files DSP 65.8 / CLAP 71.3 / CLAP+DSP 76.0 % (ECE 1.8 %, partly in-sample). Golden n/a (≥ 90 % open).** Other fixture metrics unchanged. Reviewer APPROVE round 2. Later: 5 T5.3 items.
- 2026-11-12 — T5.2: `sonar_core::embed` (feature `embed`, CLI only; FFI uses stubs until T5.6). CLAP front end per ADR-026, a pool of N single-thread int8 `ort` audio sessions (CPU default, `--ep coreml` opt-in with CPU fallback), an fp32 text encoder plus tokenizer, a 512-d embedding store, and an `Embed` job stage (lowest priority; skipped if the model is missing). CLI: `embed`, `embed-text`, `bench embed`. ADR-027. **M4 Pro, release, 10 sessions: 154.5 files/s end to end (AC ≥ 100 met); model alone 157 clips/s; load 4 s.** Fixture eval unchanged. Reviewer APPROVE in round 1; I fixed 3 trivial NITs. Later: 7 T5.2 items.
- 2026-11-12 — T5.1: CLAP model = LAION `clap-htsat-unfused` (Apache-2.0 weights, commercial redistribution OK with notice) via pinned Xenova ONNX export (ADR-026); `scripts/fetch-models.sh` (pinned commit, SHA-256, idempotent) → `models/clap-htsat-unfused/` 538 MB. **int8 audio / fp32 text: zero-shot fixtures 6-class 0.984, silver 7-class 0.631 (fp32 0.638), audio cosine vs fp32 ≥ 0.995; M4 Pro CPU 10×1-thread sessions 158 clips/s, text 4.8 ms/query** → >100 files/s reachable. Front end spec (48 kHz, 64 Slaney mel) in ADR. Reviewer APPROVE round 1 (3 trivial NITs fixed). No eval (no analysis code). Later: 5 T5.1 items.