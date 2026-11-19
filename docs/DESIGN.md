# Sonar — Design

How Sonar looks and feels. `CLAUDE.md` describes behaviour of the team,
`PRODUCT.md` what the app does, this file what it looks like. Every UI task
reads this file first and uses **only** the tokens defined here
(`app/Sonar/DesignSystem/`). ADR-039 records the decision.

## 1. Direction

**A precise instrument, native to the Mac.** Sonar is a pro audio tool: dense,
calm, fast to scan, keyboard-first. The reference mix is Sononym's information
density, Ableton's browser discipline and Linear's polish, on a standard macOS
frame. **Logic Pro's browser (macOS 26/27) is the main visual reference**
(founder, 2026-11-20, ADR-042) for the filter panel, chips and tags: filters
as colour-coded chip clouds grouped in titled sections on a floating glass
panel, each active filter a coloured chip of its own. The founder's
screenshot is `design/reference/logic-pro-browser.png` (local only,
git-ignored). Borrow structure, spacing and the colour logic; never Apple's
icons, artwork or names.

- **System frame, custom content.** Window chrome (sidebar, toolbar, inspector
  container, menus, sheets, popovers, settings) is standard SwiftUI/AppKit, so
  it gets the current macOS look (Liquid Glass on macOS 26+) for free. Sonar's
  own identity lives in the content layer: results table, player header,
  waveform, chips, meters.
- **Glass for floating controls, flat for content** (Apple HIG, vendored as
  `.claude/skills/mac-design`). Glass belongs to the navigation and control
  layer: toolbar, sidebar, popovers (the filter panel), floating buttons.
  Rows, waveforms, cards and chips never get glass. Never glass on glass.
- **Light and dark are equal citizens.** The app follows the system appearance;
  there is no in-app theme switch. Every colour is a token with a light and a
  dark value. Every screen is checked in both.
- **Content is flat.** No glass, blur, gradients or shadows in the content
  layer. Depth comes from three surface levels and hairlines.
- **Colour is information.** Neutral surfaces; the accent marks "live" things
  (playhead, waveform, selection, active filter); category hues help scanning.
  Nothing is coloured for decoration.
- **Dense, not cramped.** Desktop density: 24 pt rows, 12–13 pt text, compact
  bars. Never the airy single-column look of a phone app.

## 2. Tokens

Source of truth in code: `Theme` (`app/Sonar/DesignSystem/Theme*.swift`) and
the colour sets in `Assets.xcassets/Colors/`. The tables below must match the
code; change both together.

### 2.1 Colour

Text uses the system semantic colours (`.primary` / `.secondary` /
`.tertiary`, `NSColor.labelColor` …). Status goes through
`Theme.Color.error` / `warning` / `ok` (aliases of the system red / orange /
green, so they adapt to appearance and accessibility settings). Everything
else:

| Token | Light | Dark | Use |
|---|---|---|---|
| `accent` (`AccentContent`) | `#007A70` | `#2DD4BF` | content accent: waveform, playhead, selection tint, active chip, accent-coloured text and icons |
| `controlAccent` (`AccentColor`) | `#007A70` | `#11827A` | the app's system accent: fill of default/prominent buttons, menu highlight, sliders, focus ring |
| `surfaceContent` | `#FFFFFF` | `#17181A` | results table, main content background |
| `surfaceRaised` | `#F4F5F7` | `#1E2023` | filter bar, similarity bar, player header, cards |
| `surfaceWell` | `#E9EBEE` | `#0E0F11` | waveform well, inset areas, meter tracks |
| `hairline` | `#000000` 10 % | `#FFFFFF` 9 % | 1 px separators and borders |
| `favorite` | `#E5484D` | `#FF6369` | filled heart |
| `catDrum` | `#C2570C` | `#FF9F45` | kick, snare, clap, tom, drums |
| `catMetal` | `#A16207` | `#F5C84B` | hat, cymbal, perc |
| `catBass` | `#7C3AED` | `#A78BFA` | bass |
| `catTonal` | `#2563EB` | `#6AA8FF` | lead, pad, keys, pluck, synth |
| `catVocal` | `#DB2777` | `#F472B6` | vocal |
| `catFX` | `#64748B` | `#94A3B8` | fx, texture, anything else |

Derived (computed in `Theme`, not separate assets):
- `selection` = `accent` at 16 % (light) / 24 % (dark): focused selected row.
- `selectionInactive` = label colour at 8 %: selected row, table not focused.
- `accentSoft` = `accent` at 14 %: toggle background and the plain (unhued) active chip.
- `panelGroup` = the system quinary fill: section groups in the filter panel.
- Category chip/dot background = the category hue at 16 %, text = the hue.

Facet hues and fills (ADR-042). Every chip in
the filter panel and every active filter chip takes its section's hue.
Category values use their category hue (above). Two more hues, chosen away
from every category hue and from `accent`:

| Token | Light | Dark | Use |
|---|---|---|---|
| `facetProperty` | `#0369A1` | `#38BDF8` | Key, Tempo, Duration chips (Logic's "Properties") |
| `facetTag` | `#4D7C0F` | `#A3E635` | tag chips everywhere |

Each of the eight hues has a deep **fill** for the selected state, the
same value in light and dark, white text on it ≥ 4.5:1 (token test):

| Fill token | Value | White on it |
|---|---|---|
| `catDrumFill` | `#C2410C` | 5.2 |
| `catMetalFill` | `#A16207` | 4.9 |
| `catBassFill` | `#6D28D9` | 7.1 |
| `catTonalFill` | `#1D4ED8` | 6.7 |
| `catVocalFill` | `#BE185D` | 6.0 |
| `catFXFill` | `#475569` | 7.6 |
| `facetPropertyFill` | `#0369A1` | 5.9 |
| `facetTagFill` | `#4D7C0F` | 5.0 |

The `…Fill` tokens are the one allowed exception to "a filled custom control
uses `controlAccent`" (§5): they are never `accent` and are contrast-tested.

Category → hue goes through `Theme.categoryColor(_:)` only. Unknown or missing
category: no dot, text in tertiary.

### 2.2 Type

SF Pro (system font) only. Four steps plus one label style:

| Token | Size / weight | Use |
|---|---|---|
| `title` | 15 semibold | inspector name, empty-state title, player title |
| `body` | 13 regular | default controls, inspector values, sidebar |
| `row` | 12 regular | table cells, chips, bar controls |
| `caption` | 11 regular | secondary info, counts, hints |
| `label` | 10 semibold, uppercase, +0.6 tracking | section labels, table header, readout captions |

Numbers that change or line up (BPM, duration, counts, pitch, percentages)
always use monospaced digits (`Theme.Font.rowNumeric`, `.monospacedDigit()`),
right-aligned in columns. No other font family, no italic, no extra weights.

### 2.3 Space, radius, size

- **Space** (pt): `2, 4, 6, 8, 12, 16, 24, 32` → `Theme.Space.s2 … s32`.
  Nothing else. Inside a bar: 8 between controls, 12 at the edges. Between
  sections: 16. Around empty states: 32.
- **Radius**: `small` 4 (chips, badges, row selection), `medium` 6 (controls,
  wells), `large` 10 (cards, drop overlay).
- **Sizes**: table row 24 · table header 22 · bar height 36 · player header:
  transport row 36 + waveform 180 (user-resizable 120–360 via a split
  divider; compare mode adds a 72 pt target well above) · icon in rows/bars 12–13 pt
  (`.imageScale(.small)`), transport button 28, category dot 6
  (`Theme.Size.categoryDot`), waveform well `Theme.Size.waveformHeight` /
  `waveformMinHeight` / `waveformMaxHeight` (180 / 120 / 360), compare-mode
  target well 72 + caption row 22 (`compareWellHeight`, `compareLabelHeight`),
  aspect slider width 48 (`aspectSliderWidth`), split handle 6
  (`splitHandle`), unplayed waveform opacity `Theme.waveformUnplayedOpacity`
  0.45, knob 26 with a 2.5 arc (`Theme.Size.knob`,
  `knobStroke`).
- **Hairlines** are 1 physical pixel (`Theme.hairlineWidth`).

### 2.4 Motion

Almost none. Selection, sort and filter changes are instant. Allowed: system
sidebar/inspector animations; 120 ms ease-out fade for banners and hover
states; the playhead follows audio via display link. No springs, no bounces.

## 3. Layout

```
┌──────────┬───────────────────────────────────────────┬────────────┐
│ Sidebar  │ Player header: transport row (36)         │ Inspector  │
│ (system) │  ▶ ■ ⟳ auto · name · pitch · BPM KEY LEN  │ (system    │
│          ├───────────────────────────────────────────┤ container, │
│          │ [compare mode only] target waveform (72)  │ Sonar      │
│          │ Selected waveform (180, resizable)        │ content)   │
│          ├───────────────────────────────────────────┤            │
│          │ Filter bar (36)                           │            │
│          ├───────────────────────────────────────────┤            │
│          │ Similarity bar (only in similarity mode)  │            │
│          ├───────────────────────────────────────────┤            │
│          │ Results table (surfaceContent, 24 pt rows)│            │
└──────────┴───────────────────────────────────────────┴────────────┘
```

- **Sidebar**: standard `.sidebar` list. SF Symbols, counts as badges, offline
  libraries dimmed with a warning symbol. "Add…" actions live in the section
  header (a `+` button), not as fake rows. Library rows carry their file
  count as a badge; an offline library has a `warning` triangle icon and
  tertiary text.
- **Toolbar**: search field (centre/trailing), trial badge (small bordered
  button, `caption`), indexing status (small progress bar
  `Theme.Size.indexingProgressWidth` 100 + `caption` monospaced counter +
  pause/resume), inspector toggle. No custom-drawn toolbar items.
- **Filter bar**: ONE row, `surfaceRaised`, hairline below. Kind as a
  compact segmented control (All / Loops / One-shots); one **Filters**
  button (`line.3.horizontal.decrease`, accent when any filter is on) that
  opens the filter panel; then one **active filter chip per selected value**
  (`Kick`, `Bass`, `Am`, `120–128 BPM`, tag `dark`) in its section hue,
  filled style (§4.2), each with its own `×` (clicking the chip body opens
  the panel); Favorites as an icon toggle; "Clear all"; result count right,
  `caption`, monospaced digits. The chips scroll horizontally when the
  column is narrow; Favorites, "Clear all" and the count stay fixed on the
  right. Library is not in the bar (the sidebar already filters by library).
- **Filter panel** (T11.1, ADR-042): a system popover from the Filters
  button (glass on macOS 26+, nothing custom behind it), width
  `Theme.Size.filterPanelWidth` (480), scrolls past
  `filterPanelMaxHeight` (640). Title "Filters" + close. Sections, each a
  `large`-radius group filled with `Theme.Color.panelGroup` (a semantic
  quinary fill, so the glass shows through) with a `label` title and, when
  it holds more than 12 chips, a trailing "Show More" / "Show Less" text
  button (selected chips are never hidden by it): **Category** (chips
  ordered by family: Drums, Percussion & metal, Bass, Tonal, Vocal, FX,
  each in its category hue; multi-select), **Key** (Any / Major / Minor
  segmented control, the 12 roots as multi-select chips, then "Include
  compatible keys" and "Include uncertain"; root `A` adds `A` in Major,
  `Am` in Minor, both in Any; the bar shows one chip per root: `A`, `Am`,
  or `A any`; counts show in Major / Minor only, since in Any a mode-less
  key counts toward both modes), **Tempo** and **Duration** (min / max
  fields plus single-select quick-range chips: one range each, choosing it
  sets min/max, choosing it again clears it, typing a different bound
  deselects it; the bar shows one chip for the range), **Tags**
  (multi-select, most used first). Category, Key and Tag chips show their
  count (`caption`, monospaced) and are disabled at count 0 unless
  selected. A click applies at once (no Apply). Keyboard: Tab between
  controls, arrows between chips, Space toggles, Esc closes.
- **Results table**: see §4.1.
- **Player header** (founder request 2026-11-17, ADR-040; modelled on how
  pro sample browsers work, not on any one product's look): always present
  at the **top** of the content column (stable layout), on `surfaceRaised`,
  hairline below. Row 1 (36 pt): transport (play/pause, stop, loop,
  auto-play), file name (`title`, middle truncation), pitch knob, readouts
  (BPM, KEY, LENGTH). Below: the **big waveform** in a `surfaceWell`, full
  width, 180 pt by default, resizable with a split divider (120–360,
  remembered). Waveform interactions in §4.4 (seek, region selection, play
  selection, find similar to selection). With no selection: controls
  disabled, well shows a flat centre line.
- **Compare mode** (similarity mode only): a compact 72 pt well above the
  selected waveform shows the **similarity target** ("Similar to: name",
  `caption`, with its own small play button; the selected region is drawn
  if the search used one). The large well below shows the browsed row
  ("Selected: name"). Leaving similarity mode removes the target well.
  The similarity bar is one 36 pt row on `surfaceRaised` (target name,
  five compact aspect sliders, icon-only Reset and Close). The drop overlay
  (audio file dragged onto the results) is an `accentSoft` fill with an
  accent hairline border, radius `large`, and a centred caption; no blur.
- **Inspector**: metadata and editing only (no transport, no waveform): header
  (name `title`, folder path `caption`), actions (favorite, find similar,
  reveal), then sections "Analysis" (BPM / key / category / kind with
  overrides), "Tags", "File". Sections use a `label` heading and a two-column
  grid (label secondary left, value right); not a grouped settings `Form`.
- **Window**: minimum 900×520. When narrow, hide the inspector first, then
  collapse the sidebar; the table and player header never disappear (the waveform shrinks to
  its 120 pt minimum first).

## 4. Components

One implementation each, in `app/Sonar/DesignSystem/Components/`. Use the
system control when one exists; write a component only for the things below.

### 4.1 Results table (AppKit)
- Background `surfaceContent`; **no** alternating rows, no grid lines.
- Header: `label` style, secondary colour, 22 pt high, hairline below.
- Row 24 pt. Selection: rounded (`small`) tint `selection`, inset 4 pt
  horizontally; text keeps its normal colour (never white-on-accent).
  Unfocused table: `selectionInactive`.
- Columns: favorite (heart; outline only on hover or when set) · Name
  (`row`, primary, middle truncation) · Category (dot in category hue + name)
  · Kind (symbol + text, secondary) · BPM, Duration (`rowNumeric`,
  right-aligned) · Key (`row`, left-aligned, note + superscript cents or key, see "Key / note text" below) ·
  Library (secondary). A missing value shows nothing, not "-"; the one
  exception is Key, which shows an en dash in tertiary (silence / no value,
  ADR-041).
- Hidden files: whole row at tertiary. Loading rows: a short neutral skeleton
  bar in the Name column, not the word "Loading…".

**Key / note text (ADR-041, T10.8; `KeyLabel`).** One rendering everywhere
(table cell, KEY readout, inspector Key row). A pitched one-shot without a key override shows its
note with octave ("E2") followed by the rounded cents as a small superscript
("+35", "\u2212" for negative; omitted when it rounds to 0 or there are no
cents), Sononym style. Anything else shows its key ("D#m", "C"); no value is
an en dash in tertiary. Main text is primary, the superscript secondary
(`Theme.Font.superscript` 9 pt, `Theme.Size.superscriptBaseline` 4 pt raise).
All values share one colour: certainty is not coloured. The tier
(`Theme.KeyTier`, bounds from the core `keyTierHigh()` / `keyTierMedium()`)
only appears as the inspector's confidence word and in the "Include
uncertain" filter. The value is one line (tail truncation, never wrapped);
the table's Key column min width is the widest value (`G#4` + `+50`, `A#4` + `−45`,
`KeyLabel.widestWidth()`) plus the two `s8` cell insets.

### 4.2 Chip (`SonarChip`)
Height 22, radius `small`, `row` font, 8 pt horizontal padding. States:
default (hairline border, secondary text) · hover (`surfaceWell`) · active
(`accentSoft`, accent text, no border) · disabled (40 % opacity). Optional
count in `caption` monospaced digits; optional leading category dot;
optional trailing clear `×` (own button and action, active chips only).

**Hued chips (ADR-042, Logic Pro style).** A chip can carry a facet hue
(§2.1) through `SonarChip(hue:)`. Unselected: hue text on the hue at 16 %,
no border. Hover: hue at 24 %. Selected / active: the hue's deep fill
(`…Fill`), white text, same weight as unselected (no semibold: a chip
cloud must not reflow on toggle); an active chip's `×` is white. The count
is the label at `Theme.chipCountOpacity` (0.7). Used for every chip in the
filter panel and the bar's active filter chips; tag chips everywhere (T11.2)
use `facetTag`. One component, not a second chip. Chip clouds wrap with
`FlowLayout` (spacing `s6`).

### 4.3 Readout (`Readout`)
A value with a caption above it: caption in `label`, value in 15 pt medium
monospaced digits. Used in the player header (BPM, KEY, LENGTH, PITCH). Missing
value: an en dash in tertiary. The KEY value is note + superscript cents or key (`KeyLabel`, §4.1), primary.

### 4.4 Waveform
Bars in `accent` on `surfaceWell`, radius `medium`. Played part full accent,
unplayed part accent at 45 %. Playhead: 1 pt line in label colour. Loading:
flat centre line; error: caption text centred in the well.
- **Click** seeks (and starts playback if auto-play is on). **Drag**
  selects a region: `selection` tint over the region, 1 pt accent edges
  with small drag handles (`Theme.Size.regionEdge`, `regionHandleWidth` /
  `regionHandleHeight`; edges stay draggable), start/length readout in
  `caption` monospaced digits in a small floating bar on the region's top
  edge (`surfaceRaised`, `regionBarWidth`, with the actions menu). Regions
  are at least 50 ms (a shorter drag is a click). Esc, a click outside, or a
  different sample clears it.
- With a region: play plays the region (loops it if loop is on); the
  context menu and a small floating button on the region offer **Play
  Selection** and **Find Similar to Selection** (⌘⇧F uses the region when
  one exists, else the whole file).
- Zoom (pinch / ⌘-scroll) and pan (horizontal scroll) as before.

### 4.5 Section header (`SectionLabel`)
`label` style text, secondary colour, 16 above / 6 below.

### 4.6 Empty and error states
Centred: SF Symbol 28 pt in tertiary, `title`, one `body` line in secondary,
at most one primary action. Same component everywhere (`EmptyStateView`):
onboarding, the license gate, the engine-failed screen, empty libraries and
duplicates all use it. Quieter content under the action (secondary buttons
such as Quit / Reveal Data Folder, a drop hint) goes in its `extra` slot.
A folder drop on onboarding shows the `DropOverlayView` look (§3).

### 4.7 Banner
One line on `surfaceRaised`, leading status symbol in the system status
colour, `row` text, trailing action or dismiss. Never a tinted full-width
background.

### 4.8 Knob (`Knob`)
DAW-style rotary control for stepped values (preview pitch). 270° arc in
quaternary label colour, the part between the default and the value in
`accent`, a `surfaceRaised` cap with a 1 pt tertiary rim and a primary
pointer line. Vertical drag (6 pt per step,
up = more), double-click resets to the default, arrow keys / VoiceOver
adjust. Never paired with a Reset button; the value is shown as text beside
it (secondary when at the default).

### 4.9 Property row (`PropertyRow`)
Inspector grid row, at least 24 pt high: label in `row` secondary on the left
(`Theme.Size.propertyLabelWidth`, 72), value or editor right-aligned in the
rest. Read-only values are selectable `row` text (monospaced digits for
numbers); missing value: an en dash in tertiary. Sections are a `SectionLabel`
plus rows with 2 pt between them. The Analysis "Key" value is the `KeyLabel`
rendering (§4.1); clicking it switches to the text field (Return commits,
losing focus cancels), a trailing revert button clears the override; below it (one sample selected) read-only secondary rows give
"Key source" (`Audio` / `Name` / `Audio + Name` / `You`, then the
confidence word `High` / `Medium` / `Low`, e.g. `Audio · High`), "Name says"
(the filename's key, only when it disagrees with the audio key), "Audio says"
(the audio's key, only when the name outweighed a disagreeing audio key); a
one-shot's note is in the Key row itself. The Key section of the filter panel has "Include uncertain" (low-tier
keys) next to "Include compatible keys". Removable tags are `SonarChip`s with a
trailing `xmark`.

### 4.10 Status badge (`StatusBadge`)
`caption` text in a status colour (`Theme.Color.ok` / `warning` / `error`) on
the same colour at 16 %, radius `small`, 6 / 2 padding. Online / offline
libraries (Settings), exact / near duplicate groups.

### 4.11 Settings and sheets
Settings tabs are system grouped `Form`s (`.formStyle(.grouped)`), hints in
`caption` secondary, errors in `Theme.Color.error`. The Libraries tab is a
list plus a button row. Sheets (Enter License) are standard sheets with a
`title` heading, a `surfaceWell` text well with a hairline border, and
Cancel / default-action buttons.

**Tag editor sheet (T10.9, `TagEditorView`).** Row context menu / Edit menu
"Edit Tags…" and the inspector's tag button. A `tag` symbol + `title`
heading; a token box (`surfaceWell`, hairline, radius `medium`) holding the
tags every target file has as `SonarChip`s with `×` and a borderless inline
"Add tag" field at the end of the flow; below, a `surfaceContent` list of the
library's tags filtered by the typed text (checkmark when chosen, name, file
count in `caption` secondary, `selection` tint on the highlighted row, a
"Create “x”" row first when there is no exact match). Keys: ↓/↑ highlight,
Return toggles / creates, comma or Tab commits the text, ⌫ on an empty field
removes the last chip, Esc cancels, ⌘Return (or Return on an empty field) saves (disabled without a
change). Save applies the added / removed diff to all targets in one refresh;
with several files, tags only some have are neither shown nor removed.

## 5. Rules

Do
- Use `Theme.*` for every colour, size, space, radius and font.
- Use system controls (Button, Toggle, Slider, Picker, Menu, popover, sheet,
  alert) with `.controlSize(.small)` inside bars.
- Design all four states of a screen: empty, loading, error, filled.
- Keep every interactive element reachable by keyboard, with an
  accessibility label and a `.help` tooltip when it is icon-only.
- One primary action per view.

Don't
- No literal colours (`Color.red`, `.orange.opacity(…)`, hex), no literal
  paddings / radii / font sizes in feature code. `scripts/design-lint.sh`
  fails the build on them.
- No custom blur, glass, gradient, shadow or material in the content layer.
  Glass comes from system containers (toolbar, sidebar, popover, sheet);
  a custom `.glassEffect()` only on a floating control and only behind
  `if #available(macOS 26, *)` (ADR-042). Never a custom background behind a
  system bar, popover or sheet.
- No custom-drawn replacements for system chrome (title bar, sidebar,
  toolbar, scrollbars, menus).
- No text on `accent`: it is bright in dark mode (white on it is about
  1.8:1). The system draws white text on the app's accent asset (default and
  `.borderedProminent` buttons, menu highlight), which is why that asset is
  the darker `controlAccent` (white on it ≥ 4.5:1 in both appearances). A
  filled custom control uses `controlAccent`, never `accent`.
- No new component when §4 or a system control already covers it. If a style
  appears a second time, move it into `DesignSystem/` and document it here.
- No emoji or text glyphs as icons (`♥`); SF Symbols only.

## 6. Verification (agents can see)

`scripts/ui-snapshots.sh` renders the main screens with deterministic sample
data in **light and dark** to `logs/ui/<screen>-<light|dark>.png`. For every UI
task:

1. The implementing agent runs it and looks at the PNGs (Read tool) before
   reporting done; it fixes what it sees.
2. The orchestrator looks at them too and attaches the paths to the review.
3. The reviewer checks the PNGs against this checklist:
   - spacing and alignment follow the scale; edges line up across bars
   - at most four text steps on screen; numbers right-aligned, monospaced
   - one primary action; accent only on live/active things
   - light and dark both readable (text contrast ≥ 4.5:1, no lost hairlines)
   - no literal colour/size in the diff (`scripts/design-lint.sh` green)
   - empty, loading and error states exist and use the shared components
   - macOS 26/27 platform rules: `.claude/skills/mac-design/design-rules.md`
     (glass only in the control layer, concentric corners, toolbar items
     over their column, standard symbols). This file wins on a conflict.

UI tasks may use the vendored skills (`.claude/skills/`, ADR-042):
`mac-design` (platform rules, Liquid Glass, toolbars, menus), `ux-design`
(names, flow, popover vs sheet, wording), `design-from-app` (its
`measure.swift` / `compare.swift` measure a reference screenshot in
`design/reference/` against a snapshot).

A token-rule violation or a broken light/dark state is BLOCKING for UI tasks.
