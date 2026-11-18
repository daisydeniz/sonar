# Status

Living log maintained by the agents. Newest entries on top in "Log".

## Current
- Milestone: M10 honest key (M0–M7, M9 done; human-only: T3.7, T7.4, T8.1)
- Next task: T10.4b (M10 honest key, ADR-041)
- Loop state: RUNNING   <!-- RUNNING | BLOCKED | ROADMAP COMPLETE -->

## Toolchain (T0.1, 2026-11-09)
macOS 26.7.2 arm64 · Xcode 26.5 · xcodegen 2.46.0 · rustup 1.29.3 (Homebrew) ·
rustc/cargo 1.99.1 · cargo-deny 0.20.2 · claude 2.1.316 · gh: not installed (optional)

## Needs human
_(Agents list anything that requires the founder: decisions, credentials,
manual tests, labeled data. Keep each item one line with the task id.)_
- T10.4 Re-fit key tiers on real files: run `sonar eval --silver <dir>` (names carry keys); medium tier (60 % target) could not be validated on synthetic fixtures.
- T0.1 (optional) For `cargo` in a normal terminal / hand-started `claude`, add to ~/.zprofile: `export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"` (scripts and autopilot already handle it). Optional: `brew install gh`.
- T3.7 (optional) Golden set: `eval/golden/labels.csv` (299 files from `Samples/`) was pre-filled from the pack's own file/folder labels, not by ear; a 5-minute skim to fix or blank wrong cells makes it real golden. `scripts/eval-real.sh` runs silver + golden.
- T8.1 End-to-end test by the founder (after M7)
- T9.8 Manual check: on the welcome screen and the trial-ended gate, Return triggers the primary button; drop folders on the welcome screen → overlay; Settings tabs (Libraries online/offline badges, License, About) look right in light/dark.
- T9.6 Manual check: Find Similar on a sample (also with a waveform region) → 72 pt target well above the selected waveform, its play button plays the target (region only), Space stops it; Esc/× removes the well; drag a .wav from Finder → overlay readable.
- T9.4 Manual check: select one sample → edit BPM/key/category/kind override and revert, add/remove a tag; ⌘/⇧-select several → fields show common values or "Mixed", an edit or tag applies to all, Reveal shows all in Finder.
- T9.3c Manual check: select a sample, drag across the waveform, drag each edge; Space plays the region (loop on → loops it); right-click → Play Selection / Find Similar to Selection; ⌘⇧F with a region; Esc and click outside clear it.
- T9.3 Manual check: drag the divider under the waveform (resize cursor, 120–360 pt, height kept after relaunch); no selection shows a flat line and disabled transport; play/stop/loop/Auto/pitch knob in the header work.
- T9.2 Manual check (not visible in snapshots): hovering a results row shows the outline heart and clicking it favourites the row; clicking a column header sorts and shows the sort arrow next to the title; selection tint turns grey when the table loses focus.
- T9.1 The `swift-ui` agent ran `brew install xcodegen` (2.46.0) because it was missing on this Mac; `scripts/bootstrap.sh` should have covered it. Nothing to do unless you want it removed.
- T9.x Design direction check (optional, cheapest now): open `logs/ui/components-dark.png` / `-light.png` after `scripts/ui-snapshots.sh`; tell the loop if the teal accent or category hues should change (one-line token edits in `docs/DESIGN.md` §2.1).
- T7.4 Apple Developer ID, notarization, payment provider, pricing. Release env vars for `scripts/release.sh` are listed in ADR-037 (signing identity, notarytool profile or API key, Sparkle keypair + feed URL); `--dry-run` builds a local-only DMG.
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
- 2026-11-18 — T10.4: Key calibration: `sonar eval` prints accuracy per tier + 10-bin ECE (fixtures and `--silver`, names hidden; any key on drum/noise = wrong, mode-less matches on pitch class). Bounds `KEY_TIER_HIGH` 0.75 / `KEY_TIER_MEDIUM` 0.30 in `key/tier.rs`, FFI `key_tier_high()/medium()`. Fixtures: high 68/68 100 %, medium empty, low 0/36, ECE 3.74 %, monotone. Silver not readable from agent shell. New tracked `key.tier_high_acc_pct`. check.sh green; reviewer APPROVE r1. Later: 2 T10.4 items.
- 2026-11-18 — T10.3: Loop root note (`key/root.rs`): onset segments → YIN note per segment (unpitched <50 ms sustain), ≥ 60 % energy on one pitch class → mode-less key, conf = share × note conf; fused with chroma (same tonic noisy-or, else T10.2 rule). Seeded loops: root 16/16, mixed max conf 0.24, drum loops 0.09, clicks 0; fixtures key 16/16, note 52/52 unchanged. ~10 ms per 30 s loop. KEY_VERSION 4, DSP_VERSION 9, no migration. check.sh green; reviewer APPROVE r1. Later: 2 T10.3 items.
- 2026-11-18 — T10.2: Fused key with graded confidence (`key/fuse.rs`): ADR-022 gates now ramps multiplying audio confidence; name prior fused (agree → noisy-or, source both; disagree → winner·(1−loser)); every non-silent file gets a key, silence none; note confidence × cos(π·cents/100). Migration 0003 (`key_source` 'both'), KEY_VERSION 3, DSP_VERSION 8, schema 3. Fixtures key/note 100 % unchanged; drum/noise max conf 0.21 (< 0.3). Silver/golden dirs not readable from agent shell. check.sh green; reviewer APPROVE r1. Later: 4 T10.2 items.
