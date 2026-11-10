# Status

Living log maintained by the agents. Newest entries on top in "Log".

## Current
- Milestone: M3 (M0–M2 done, tags m0-done, m1-done, m2-done)
- Next task: T3.3
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
- 2026-11-11 — T3.2: `dsp::aspects` (ADR-019, `ASPECTS_VERSION` 1): raw per-file vectors Spectrum 38 / Timbre 57 (MFCC c1–c19 mean/std/Δstd, c0 dropped) / Pitch 18 (YIN f0, voicing, pitch-class, chroma) / Amplitude 22 (log-time envelope, attack/decay, crest, RMS); Overall 135 = weighted concat of z-scored aspects. Gain-invariant Spectrum/Timbre (80 dB clip). `NormStats` Welford + Chan merge, JSON, z-score std floor + ±8 clamp. Reusable `dsp::yin` (< 0.2 cent on sines). CLI `sonar aspects <file>… [--json]`. Bitwise stable across runs. **10.9 ms aspects / 14.1 ms total per 30 s file, 1 thread.** Reviewer APPROVE round 1 (2 trivial NITs fixed). Pipeline wiring moved to T4.1 note; Later: 1 T3.2 item. No eval (no detection change; T3.6).
- 2026-11-11 — T3.1: `sonar_core::dsp` (realfft, MIT/Apache): `Analyzer` (one per worker, reuses FFT plan/filterbank/DCT) → `Features {spectrogram, log_mel, mfcc, onset}`, spectrogram computed once for T3.2–T3.5. STFT n_fft 2048 / hop 512 / periodic Hann, centred; 64 Slaney mel bands 20 Hz–Nyquist, dB floor −100; orthonormal DCT-II MFCC c0–c19 + HTK deltas; onset = rectified log-mel flux (80 dB range clip). `DSP_VERSION` 1. CLI `sonar dsp <file>… [--json]`. 32 analytic-signal tests (bin/magnitude, Parseval, mel partition, DCT vs direct, click-train peaks ±1 frame, empty/NaN/huge inputs finite). **3.95 ms per 30 s file, 1 thread, release.** Reviewer APPROVE round 1. Later: 2 T3.1 items. No eval (no detection change; T3.6).
- 2026-11-11 — T2.4: `sonar bench search [--docs --rounds --seed --dir --keep --json]` (seeded 500k corpus → real store → production on-disk index, reopened; warmup untimed; every query with facets; exits non-zero if any class p95 ≥ 50 ms). **Release, M4 Pro, 500k:** build 1.78 s, 34.4 MB; first query after reopen 6.1 ms; p95 ms: as-you-type 29.3 (tail = 1-letter prefix `s`, ~32 ms), text+filters 4.3, browse-all+facets by name 27.7 / by BPM 29.4 (≈ max, 30 samples), range+facets 9.7. **Budget p95 < 50 ms met**; no search code changed. Reviewer APPROVE round 1. Later: 5 T2.4 items. No eval (no text-query set). M2 complete (m2-done).
