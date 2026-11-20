# Status

Living log maintained by the agents. Newest entries on top in "Log".

## Current
- Milestone: M12 Visual v2 and tags UX (founder request 2026-11-20, ADR-043); human-only: T3.7, T7.4, T8.1
- Next task: T12.7 Tags menu and tag popover
- Loop state: RUNNING   <!-- RUNNING | BLOCKED | ROADMAP COMPLETE -->

## Toolchain (updated 2026-11-20; T0.1 was macOS 26.7.2 / Xcode 26.5)
macOS 27.2.1 arm64 · Xcode 27.2 · xcodegen 2.46.0 · rustup 1.29.3 (Homebrew) ·
rustc/cargo 1.100.0 · cargo-deny 0.21.0 · claude 2.1.328 · gh: not installed (optional)

## Needs human
_(Agents list anything that requires the founder: decisions, credentials,
manual tests, labeled data. Keep each item one line with the task id.)_
- T12.6 Manual check (toolbar not in snapshots): window title shows the scope ("All Samples") with the libraries in scope as subtitle; trial badge amber; playing a row shows the speaker symbol in its status column.
- T12.4 Manual check: play a loop → playhead runs from the ruler down; Gain knob at −24 and +6 dB is audible, double-click resets, value survives relaunch; zoom menus and pinch zoom move the ruler; region drag still works below the ruler.
- T11.3 Manual check (snapshots have no window toolbar): the inspector toggle sits at the trailing edge over the inspector, stays reachable when the inspector is hidden, and ⌃⌘I / View > Show Inspector toggles it; the inspector background matches with and without a selection.
- T11.1 Manual check: click Filters → popover (glass on macOS 26+, height sensible, scrolls when long); Tab between sections, arrows between chips, Space toggles, Esc closes; chips apply at once; bar chip × removes one value, chip body reopens the panel; Key Any/Major/Minor maps roots to `A` / `Am`.
- T10.7 Golden labels: 3 BPM labels in `eval/golden/labels.csv` are wrong by file length ("How Long" kit stems are 122 not 120; Kick Loop 14 + Darbuka Loop 10 are 120 not 122; see ADR-021 addendum T10.7); fixing them takes golden BPM ±1 to 90.2 %. Re-run `scripts/eval-real.sh`. Libraries re-analyse once (DSP_VERSION 11).
- T0.1 (optional) For `cargo` in a normal terminal / hand-started `claude`, add to ~/.zprofile: `export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"` (scripts and autopilot already handle it). Optional: `brew install gh`.
- T3.7 (optional) Golden set: `eval/golden/labels.csv` (299 files from `Samples/`) was pre-filled from the pack's own file/folder labels, not by ear; a 5-minute skim to fix or blank wrong cells makes it real golden. `scripts/eval-real.sh` runs silver + golden.
- T8.1 End-to-end test by the founder (after M7)
- T10.9 Manual check: right-click samples → Edit Tags…: type "x" + Return creates a chip; ↓/↑ + Return toggles an existing tag; ⌫ on the empty field removes the last chip; ⌘Return saves → table/inspector tags and Tags filter counts update; Esc cancels (onKeyPress keys not covered by tests).
- T10.8 Manual check: click the inspector Key on a one-shot → field opens focused; type a key + Return → table/readout show it (not the note); revert brings E2⁺³⁵ back; ⇧-select mixed keys → table keeps keyboard focus.
- T9.8 Manual check: on the welcome screen and the trial-ended gate, Return triggers the primary button; drop folders on the welcome screen → overlay; Settings tabs (Libraries online/offline badges, License, About) look right in light/dark.
- T9.6 Manual check: Find Similar on a sample (also with a waveform region) → 72 pt target well above the selected waveform, its play button plays the target (region only), Space stops it; Esc/× removes the well; drag a .wav from Finder → overlay readable.
- T9.4 Manual check: select one sample → edit BPM/key/category/kind override and revert, add/remove a tag; ⌘/⇧-select several → fields show common values or "Mixed", an edit or tag applies to all, Reveal shows all in Finder.
- T9.3c Manual check: select a sample, drag across the waveform, drag each edge; Space plays the region (loop on → loops it); right-click → Play Selection / Find Similar to Selection; ⌘⇧F with a region; Esc and click outside clear it.
- T9.3 Manual check: drag the divider under the waveform (resize cursor, 120–360 pt, height kept after relaunch); no selection shows a flat line and disabled transport; play/stop/loop/Auto/pitch knob in the header work.
- T9.2 Manual check (not visible in snapshots): hovering a results row shows the outline heart and clicking it favourites the row; clicking a column header sorts and shows the sort arrow next to the title; selection tint turns grey when the table loses focus.
- T9.1 The `swift-ui` agent ran `brew install xcodegen` (2.46.0) because it was missing on this Mac; `scripts/bootstrap.sh` should have covered it. Nothing to do unless you want it removed.
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
- 2026-11-20 — T12.6: results table v2 (selection inset 6, tertiary header, playing row shows `speaker.wave.2` in `accent`), toolbar title = scope + libraries subtitle (`ScopeLabel`), trial badge `signal` on `signalSoft`; inspector header with three equal actions and `InspectorCard`s Analysis / Tags / File (token `cardPaddingVertical`). Bit depth/size not in core → Later. UI only, no eval. check.sh green; reviewer APPROVE r1. Later: 2 T12.6 items.
- 2026-11-20 — T12.5: filter bar v2 (Filters button with active count on `selection`, active chips in section hues incl. Library) and panel v2 (centred title + round close, Favorites quick card, `surfaceGroup` cards with hue squares, new Library section from the `libraries` facet, multi-select OR, footer "n results · m filters" + Clear all). No core change (library ids already in `SearchFilters`). Tokens `hueSquare`, `panelCloseButton`. UI only, no eval. check.sh green; reviewer APPROVE r1. Later: 3 T12.5 items.
- 2026-11-20 — T12.4: player header v2 — transport row (30 pt play circle, Loop/AUTO in `signal`, name + extension, Pitch/Gain knobs), waveform well with ruler (bars.beats with BPM, seconds without; follows zoom/pan), one lane per channel, readout strip (BPM/Key/Length/Format in `signal`, zoom menus); preview gain −24…+6 dB (engine only, persisted); Swift waveform path now `[[WaveformPeak]]`, envelope merge removed. New `main-oneshot` snapshot. UI only, no eval. check.sh green; reviewer APPROVE r1. Later: 4 T12.4 items.
