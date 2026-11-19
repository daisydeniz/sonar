# Status

Living log maintained by the agents. Newest entries on top in "Log".

## Current
- Milestone: all agent milestones done (M0–M7, M9, M10); human-only: T3.7, T7.4, T8.1
- Next task: none for agents — founder: T8.1 end-to-end test, triage `## Later`
- Loop state: ROADMAP COMPLETE   <!-- RUNNING | BLOCKED | ROADMAP COMPLETE -->

## Toolchain (T0.1, 2026-11-09)
macOS 26.7.2 arm64 · Xcode 26.5 · xcodegen 2.46.0 · rustup 1.29.3 (Homebrew) ·
rustc/cargo 1.99.1 · cargo-deny 0.20.2 · claude 2.1.316 · gh: not installed (optional)

## Needs human
_(Agents list anything that requires the founder: decisions, credentials,
manual tests, labeled data. Keep each item one line with the task id.)_
- T10.7 Golden labels: 3 BPM labels in `eval/golden/labels.csv` are wrong by file length ("How Long" kit stems are 122 not 120; Kick Loop 14 + Darbuka Loop 10 are 120 not 122; see ADR-021 addendum T10.7); fixing them takes golden BPM ±1 to 90.2 %. Re-run `scripts/eval-real.sh`. Libraries re-analyse once (DSP_VERSION 11).
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
- 2026-11-19 — T10.7: BPM octave via tactus prior (120 BPM, σ 0.6 oct) + loop-length prior on stems; kind gets long-file / sustained-end / onset-count terms; leading-silence skip on long files. TEMPO 2, KIND 2, DSP_VERSION 11. Golden (names hidden): BPM ±1 32.0→88.9 %, loop→one-shot 42→3/287. Silver: BPM 25.3→81.2 %, loop→one-shot 234→49/1010 (4.85 %). Fixtures unchanged (kind 100, BPM 97.06). Kind AC met; BPM AC partially met (28 length-proven mislabels, FX/a cappella). Reviewer APPROVE r2. Later: 3 T10.7 items.
- 2026-11-19 — T10.6: Key display per tier: cells, KEY readout and inspector Key in tier colour (high primary / medium secondary / low + conf 0 tertiary), en dash for none; inspector Key source + confidence word, one-shot note with cents (≈ when |cents| > 25), "Name says"/"Audio says" on disagreement; Key popover "Include uncertain". Migration 0004 (`pitch_cents`, `key_audio`), KEY_VERSION 5, DSP_VERSION 10. Eval-real unchanged vs T10.4b (silver key high/medium 79.7/61.5 %, MIREX 34.2 %; golden 68.8/34.5 %, MIREX 37.9 %). check.sh green; reviewer APPROVE r2. Later: 3 T10.6 items (incl. ui-snapshots.sh hang).
- 2026-11-18 — T10.5: Key filter/facet honest by default: only medium + high tier (overrides count as certain); `--key-uncertain` / FFI `include_uncertain_keys` adds low. Mode-less `A` answers `A` and `Am` (also via compatible keys); sort by key puts low tier after the rest. `ff_key` = root·3+mode (+36 low), index SCHEMA_VERSION 3 (rebuild). Note: name-only keys of not-yet-analysed files are low tier, so they leave the default key filter until analysis. 500k p95 30.6 ms (budget 50). Fixtures eval green; no analysis change → no eval-real. check.sh green; reviewer APPROVE r1. Later: 2 T10.5 items.
