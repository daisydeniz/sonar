# Status

Living log maintained by the agents. Newest entries on top in "Log".

## Current
- Milestone: M9 UI redesign (founder request 2026-11-17, ADR-039, `docs/DESIGN.md`); M0–M7 done; T3.7, T7.4, T8.1 human
- Next task: T9.2 results table restyle
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
- T9.1 The `swift-ui` agent ran `brew install xcodegen` (2.46.0) because it was missing on this Mac; `scripts/bootstrap.sh` should have covered it. Nothing to do unless you want it removed.
- T9.x Design direction check (optional, cheapest now): open `logs/ui/components-dark.png` / `-light.png` after `scripts/ui-snapshots.sh`; tell the loop if the teal accent or category hues should change (one-line token edits in `docs/DESIGN.md` §2.1).
- T7.4 Apple Developer ID, notarization, payment provider, pricing. Release env vars for `scripts/release.sh` are listed in ADR-037 (signing identity, notarytool profile or API key, Sparkle keypair + feed URL); `--dry-run` builds a local-only DMG.
- T5.2 (permissions) In the T5.2 session, running `scripts/*.sh` and `core/target/*/sonar` directly needed approval (cargo commands did not), so the check.sh gates were run one by one and build-core.sh/xcodebuild were skipped (sonar-ffi unchanged). Allow `scripts/check.sh` in the autopilot's permission settings.
- T5.2 Model-dependent embed tests pass as no-ops without `models/` (weights not in git); run `scripts/fetch-models.sh` on any machine that runs check.sh, so the embed stage is actually exercised.
- T7.5 Release: `sonar license keygen --out <secret file>` (keep the secret file private), set `SONAR_LICENSE_PUBLIC_KEY` (+ `SONAR_PURCHASE_URL`, the https store page) for `scripts/release.sh`; the payment provider's webhook mints keys with `sonar license issue` (ADR-038).
- T7.5 Manual check: with a dev build, set `license.json` trial start > 14 days back (or wait) → gate shows; mint a dev key (`sonar license issue`, dev secret in `core/crates/sonar-core/dev/`) and paste it → gate gone; toolbar trial badge and Settings → License tab look right; Deactivate returns to trial/gate.
- T7.2 Manual check: first launch with an empty `SONAR_HOME` shows the welcome view; add folders via button and by dropping from Finder; ⌘, opens Settings (all four tabs); unplug a library drive → sidebar shows it offline; search for nonsense → "No results" with Clear.
- T6.3 Manual check (agents cannot see the screen): ⌘F focuses the toolbar search (also in full screen); ↓/Return in search moves to the results; Esc returns; Return reveals in Finder; filter bar layout looks right.
- T6.9b Manual check: Sidebar → Duplicates lists exact and near groups. Clicking a file previews it; Reveal opens Finder. "Keep This" hides the other members (files stay on disk), and "Show hidden" brings them back.
- T6.8 Manual check: right-click several samples → favorite / hide / add tag / add to new collection; set a BPM/key/category override in the inspector and revert it; "Show hidden" brings hidden files back.
- T6.7 Manual check: ⌘/⇧-click several samples, drag them into an Ableton Live track and into a Finder window (copies arrive, originals stay); drag a row back onto the results (no similarity search); ⌘C then ⌘V in Finder pastes the files.
- T6.6 Manual check: drag a .wav from Finder onto the results (highlight + spinner, then results; .txt gives no highlight); right-click → Find Similar and ⌘⇧F; sliders re-order results; Reset, × and Esc return to the text search.
- T6.5 Manual check: waveform shows in the inspector for each selection; pinch / ⌘-scroll zoom, horizontal scroll pans, playhead moves smoothly (also with loop + pitch), click seeks.
- T6.4 Manual listening check: auto-play starts instantly on arrow keys, loop and pitch (±12 st) are audible, loud and quiet samples sound level-matched, and switching the output device mid-preview does not break playback.

## Metrics baseline
_(Filled by eval runs; see eval/reports/BASELINE.md.)_
- Fixtures (2026-11-11, 120 files, names hidden): kind 100 %, BPM ±1 97.06 % (1 octave error / 34 loops), one-shots with BPM 0, key exact/MIREX 100 %, note PC/MIDI 100 %, unexpected decode errors 0. Similarity (2026-11-12, 75 seeds): P@10 81.51 %, MRR 99.33 %, pitch-only P@10 72.84 %. Category (2026-11-12, 56 labelled, audio only): DSP top-1 33.93 % / macro-F1 50.72 % (tracked), CLAP+DSP 53.6 %. Text/NL search (2026-11-12, 19 queries): NDCG@10 BM25 28.96 % (tracked), hybrid 91.28 %. Silver (T5.3, 7818 files): category CLAP+DSP 76.0 %. Golden: not run.

## Log
- 2026-11-17 — T9.1: Design foundation (ADR-039, founder request: pro-audio look, system light/dark). `docs/DESIGN.md` (direction, tokens, layout, components, rules); `Theme` tokens + 12 colour sets in `app/Sonar/DesignSystem/`, components (chip, readout, section label, empty state, hairline); `scripts/design-lint.sh` in check.sh (15 files pending migration); `scripts/ui-snapshots.sh` → `logs/ui/*.png` (6 screens × light/dark, borderless: no toolbar); CLAUDE.md + swift-ui/reviewer rules; ROADMAP M9 (T9.1–T9.9). Only visible change: `EmptyStateView` new look. check.sh green; reviewer round 1 CHANGES (white text on bright dark accent → split `accent` / `controlAccent` + contrast test), 3 NITs fixed. Later: 2 T9.1 items.
- 2026-11-14 — T7.5: Licensing (ADR-038). Core `sonar_core::license`: offline Ed25519-signed keys `SONAR1-…`, provider-agnostic `LicenseVerifier` trait, 14-day trial in `license.json` (clock-rollback guard, atomic writes, corrupt file → trial from mtime). CLI `sonar license keygen|issue|status|activate|deactivate`; FFI free fns (work without the engine). App: `LicenseService` protocol + `LicenseManager`, trial badge, Enter License sheet (menu/badge/gate/Settings), Settings → License tab, blocking gate on expiry (Enter / Buy / Quit), fails open on errors. release.sh refuses a real build without a valid, non-dev public key. 8 XCTests + Rust/CLI tests. check.sh green; reviewer APPROVE round 1, 4 NITs fixed. No analysis/search change (no eval). Later: 2 T7.5 items.
- 2026-11-14 — T7.3: `scripts/release.sh` (ADR-037): universal Rust core + app (arm64+x86_64; `--arch arm64` fallback), inside-out hardened-runtime codesign incl. Sparkle helpers, notarytool (profile or API key, fails unless Accepted) + staple, versioned DMG in `dist/`, Sparkle 2.10.1 (SPM) with "Check for Updates…" inert unless feed URL + public key are set, EdDSA-signed appcast. All credential steps env-driven, skipped with warnings. `--dry-run` verified here: lipo x86_64+arm64, codesign strict ok, hdiutil verify + mount ok. 4 XCTests (`AppUpdaterTests`). check.sh green; reviewer APPROVE round 1, 6 NITs fixed. Later: 3 T7.3 items.
