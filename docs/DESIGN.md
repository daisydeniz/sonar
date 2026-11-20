# Sonar — Design

How Sonar looks and feels. `CLAUDE.md` describes behaviour of the team,
`PRODUCT.md` what the app does, this file what it looks like. Every UI task
reads this file first and uses **only** the tokens defined here
(`app/Sonar/DesignSystem/`). ADR-039 records the decision, ADR-043 the
v2 look (founder, 2026-11-20).

## 1. Direction

**A precise instrument, native to the Mac.** Sonar is a pro audio tool: dense,
calm, fast to scan, keyboard-first. **Visual v2** (founder, 2026-11-20,
ADR-043) takes its look from Logic Pro: flat, high-contrast surfaces, a blue
waveform with a time ruler, amber for "on" states and values, colour-coded
chip clouds in titled sections. Borrow structure, spacing and the colour
logic; never Apple's icons, artwork or names.

**Target pictures**: the approved mockups, light and dark, live in
`design/reference/sonar-v2/*.png` (local, git-ignored; the founder's canvas
"Sonar — Arayüz Konsepti" is the source). Every UI task opens the PNGs for its
screen and compares its snapshots to them. Where a mockup and this file
disagree, this file wins; where this file is silent, match the mockup.
Mock data in the PNGs (pack names, counts) is illustration only.

- **System frame, custom content.** Window chrome (sidebar, toolbar, inspector
  container, menus, sheets, popovers, settings) is standard SwiftUI/AppKit, so
  it gets the current macOS look (Liquid Glass on macOS 26+) for free. Sonar's
  own identity lives in the content layer: player header, waveform, results
  table, chips, cards, knobs.
- **Flat and opaque content, glass only from the system.** The founder found
  the glassy concepts "too liquid": content is flat with clear contrast. No
  custom glass, blur, gradient or shadow in the content layer. A system
  popover may be glass, but what sits in it (filter sections, tag lists) is on
  opaque `surfaceGroup` cards so it reads like Logic's panel. Never glass on
  glass.
- **Separate with surfaces, not lines.** A logical group is its own surface
  (a `surfaceGroup` card, radius `large`) with space around it. Hairlines only
  between bars and under table headers; never a line to split a card.
- **Light and dark are equal citizens.** The app follows the system appearance;
  there is no in-app theme switch. Every colour is a token with a light and a
  dark value; light is designed on its own (deep hues on pale surfaces), not
  dark inverted. Every screen is checked in both.
- **Colour is information.** Neutral graphite surfaces. Blue `accent` marks
  live things (waveform, playhead, selection, Filters button). Amber `signal`
  marks switched-on modes (Loop, Auto) and the readout values under the
  waveform. Each filter section has one hue (§2.1) used by its chips
  everywhere. Knob faces carry their parameter family colour. Nothing is
  coloured for decoration: no colour dot that means nothing.
- **Dense, not cramped.** Desktop density: 26 pt rows, 12–13 pt text, compact
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
else (v2 values, ADR-043; the teal accent and the per-category hues of v1 are
gone):

| Token | Light | Dark | Use |
|---|---|---|---|
| `accent` (`AccentContent`) | `#0A62D0` | `#52A8FF` | content accent: playhead head, selection tint, Filters button, links, accent icons |
| `controlAccent` (`AccentColor`) | `#0A62D0` | `#2F6FD0` | the app's system accent: play button fill, default/prominent buttons, menu highlight, focus ring (white on it ≥ 4.5:1) |
| `signal` | `#8A5600` | `#F5C451` | amber: text/icon of an "on" mode toggle (Loop, Auto), readout values under the waveform, trial badge |
| `surfaceContent` | `#FFFFFF` | `#17171A` | results table, main content background |
| `surfaceRaised` | `#F7F7F9` | `#202023` | player header, filter bar, similarity bar |
| `surfaceGroup` | `#EEEEF1` | `#2A2A2E` | cards: inspector sections, filter panel sections, tag popover list well |
| `chipBase` | — (hue at 11 %) | `#1D1D21` | unselected chip background (dark); light uses the chip's hue at 11 % |
| `waveWell` | `#EDF3FC` | `#0D1B2E` | waveform well background |
| `waveStrip` | `#E3ECF8` | `#0A1524` | readout strip at the bottom of the well |
| `wave` | `#1D6BDB` | `#52A8FF` | waveform bars, ruler ticks and labels, zoom icons |
| `hairline` | `#000000` 9 % | `#FFFFFF` 7 % | 1 px separators and borders |
| `favorite` | `#E5484D` | `#FF6369` | filled heart |

Derived (computed in `Theme`, not separate assets):
- `selection` = `accent` at 12 % (light) / 20 % (dark): focused selected row,
  Filters button background, highlighted list row.
- `selectionInactive` = label colour at 8 %: selected row, table not focused.
- `signalSoft` = `signal` at 18 % (light) / 16 % (dark): background of an "on"
  toggle and of the trial badge.
- `waveGrid` = `wave` at 10 % (light) / 14 % (dark): lane divider, well
  border. `waveCentre` = `wave` at 28 % / 32 %: lane centre line.
  Unplayed bars = `wave` at `Theme.waveformUnplayedOpacity` (0.40).

**Facet hues** (ADR-043, Logic's section colours). One hue per filter
section; its chips use it in the panel, in the bar and anywhere else that
value appears (tags in the inspector, popover, menus). Text colour per
appearance, one deep **fill** for the selected state (same in both, white
text ≥ 4.5:1, token test):

| Hue | Text light | Text dark | Fill | White on fill | Section |
|---|---|---|---|---|---|
| `facetCategory` | `#9A1DB5` | `#E27DF5` | `#A21CAF` | 6.3 | Category (magenta) |
| `facetKey` | `#4C43D9` | `#A3A0FF` | `#4F46E5` | 6.3 | Key (indigo) |
| `facetProperty` | `#03679C` | `#5CC8F5` | `#0369A1` | 5.9 | Tempo, Duration (cyan) |
| `facetTag` | `#8F4305` | `#F7A24C` | `#B45309` | 5.0 | Tags, everywhere (orange) |
| `facetLibrary` | `#A82020` | `#FF7B78` | `#C62828` | 5.6 | Library (red) |

The `…Fill` tokens are the one allowed exception to "a filled custom control
uses `controlAccent`" (§5): they are never `accent` and are contrast-tested.
Categories have **no** per-category colour any more: the Category column is
plain text and `Theme.categoryColor(_:)` is removed.

**Knob families** (§4.8): `knobPitch` `#4C9BFF`, `knobGain` `#F2C14E`,
`knobFilter` `#E5534B`, `knobTime` `#3DBE7A`, `knobNeutral` light `#D8D8DD` /
dark `#3A3A40`. Same face value in both appearances (they are physical-looking
caps); the pointer is black at 55 % (white on `knobNeutral` dark).

### 2.2 Type

SF Pro (system font) only. Four steps, one label style and two emphasis
variants:

| Token | Size / weight | Use |
|---|---|---|
| `title` | 15 semibold | inspector name, empty-state title, player file name |
| `body` | 13 regular | default controls, inspector values, sidebar, menus |
| `row` | 12 regular | table cells, bar controls |
| `chip` | 12 medium | every chip (same weight selected or not: no reflow) |
| `caption` | 11 regular | secondary info, counts, hints, card titles (semibold) |
| `label` | 10 semibold, uppercase, +0.6 tracking | table header, knob captions |
| `readoutValue` | 13 medium, monospaced digits | values in the waveform readout strip, knob values |

Numbers that change or line up (BPM, duration, counts, pitch, percentages)
always use monospaced digits (`Theme.Font.rowNumeric`, `.monospacedDigit()`),
right-aligned in columns. No other font family, no italic, no extra weights.

### 2.3 Space, radius, size

- **Space** (pt): `2, 4, 6, 8, 12, 16, 24, 32` → `Theme.Space.s2 … s32`.
  Nothing else. Inside a bar: 8 between controls, 12 at the edges. Between
  cards: 8. Between sections: 16. Around empty states: 32.
- **Radius**: `small` 4 (badges, row selection), `medium` 6 (chips, controls,
  wells' inner parts), `well` 8 (waveform well), `large` 10 (cards, drop
  overlay). Popovers and sheets keep the system radius.
- **Sizes**: table row 26 · table header 24 · bar height 44 (filter bar,
  similarity bar) · chip 24 (22 inside menus/lists) · player header:
  transport row 48 + waveform well (ruler 22 + lanes `waveformHeight` 150,
  user-resizable 96–320 via the split divider + readout strip 34) ·
  compare mode adds a 72 pt target well above · transport button 28, play
  button 30 (circle) · knob 24 in bars, 44 in panels
  (`Theme.Size.knobSmall` / `knobLarge`, value arc 2.2 outside the face) ·
  icon in rows/bars 12–13 pt (`.imageScale(.small)`) · aspect slider width 48
  (`aspectSliderWidth`), split handle 6 (`splitHandle`) · tag popover width
  340 · Manage Tags sheet 580 × up to 640.
- Also tokens (T12.4): `laneDivider` 2, `zoomButtonWidth`/`Height` 26 × 22,
  `playheadHead` 8 (ruler triangle), `rulerHeight` 22, `readoutStripHeight` 34,
  `transportRowHeight` 48, `Theme.pressedOpacity` 0.7 / `disabledOpacity` 0.38
  (transport buttons). Transport look: `PlayCircleButtonStyle`,
  `TransportButtonStyle`, `TransportToggleStyle` (DesignSystem/Components).
  The two zoom buttons are menus (horizontal: in / out / whole file;
  vertical: amplitude ×2 / ÷2 / reset, display only).
- **Hairlines** are 1 physical pixel (`Theme.hairlineWidth`).

### 2.4 Motion

Almost none. Selection, sort and filter changes are instant. Allowed: system
sidebar/inspector animations; 120 ms ease-out fade for banners and hover
states; the playhead follows audio via display link. No springs, no bounces.

## 3. Layout

```
┌──────────┬───────────────────────────────────────────┬────────────┐
│ Sidebar  │ Transport row (48): ▶ ■ ⟳ AUTO │ name … PITCH GAIN │ Inspector │
│ (system) ├───────────────────────────────────────────┤ (system    │
│ Library  │ [compare mode only] target waveform (72)  │ container; │
│ Libraries│ Waveform well: ruler 22                   │ Sonar      │
│ Collect. │   lanes (150, resizable; 2 lanes stereo)  │ cards)     │
│ Tags     │   readout strip 34: BPM Key Length Format │            │
│          ├───────────────────────────────────────────┤ header     │
│          │ Filter bar (44)                           │ Analysis   │
│          ├───────────────────────────────────────────┤ Tags       │
│          │ Similarity bar (only in similarity mode)  │ File       │
│          ├───────────────────────────────────────────┤            │
│          │ Results table (surfaceContent, 26 pt rows)│            │
└──────────┴───────────────────────────────────────────┴────────────┘
```

- **Sidebar**: standard `.sidebar` list. SF Symbols, counts as badges, offline
  libraries dimmed with a warning symbol. "Add…" actions live in the section
  header (a `+` button), not as fake rows. Library rows carry their file
  count as a badge; an offline library has a `warning` triangle icon and
  tertiary text. Sections: Library (All Samples, Favorites, Duplicates),
  Libraries, Collections, **Tags** (ADR-043): the 5 most used tags (`tag`
  symbol, name, count badge) and a "Show All (n)" row that expands the
  section in place. Clicking a tag scopes the results to it (like a library);
  dropping result rows on a tag adds that tag to them (Undo). No colour dot
  in front of tags.
- **Toolbar**: navigation title = the current scope ("All Samples"), subtitle
  the libraries in scope (`caption`). Search field (trailing), trial badge
  (`signal` text on `signalSoft`, `caption` semibold), indexing status (small
  progress bar `Theme.Size.indexingProgressWidth` 80 + `caption` monospaced
  counter + pause/resume), inspector toggle over the inspector. No
  custom-drawn toolbar items.
- **Filter bar**: ONE row, 44 pt, `surfaceRaised`, hairline above and below.
  Kind as a compact segmented control (All / Loops / One-shots); one
  **Filters** button (`line.3.horizontal.decrease`, `selection` background +
  accent text and the number of active filters when any is on) that opens
  the filter panel; then one **active filter chip per selected value**
  (`Drums`, `Am`, `120–140 BPM`, tag `warm`, library `KSHMR Vol 1`) in its
  section hue, filled style (§4.2), each with its own `×` (clicking the chip
  body opens the panel); Favorites as an icon toggle; "Clear all"; result
  count right, `caption`, monospaced digits. The chips scroll horizontally
  when the column is narrow; Favorites, "Clear all" and the count stay fixed.
- **Filter panel** (T11.1, ADR-042/043): a system popover from the Filters
  button, width `Theme.Size.filterPanelWidth` (480), scrolls past
  `filterPanelMaxHeight` (640). Title "Filters" (centred, `body` bold) +
  round close button. A first card holds the quick toggles (Favorites).
  Then one **opaque `surfaceGroup` card per section** (radius `large`,
  12 pt padding, 8 pt between cards) with its title in `body` semibold
  primary, a small square in the section hue before it, and, when it holds
  more than 12 chips, a trailing "Show More" / "Show Less" text button
  (selected chips are never hidden by it): **Category** (one hue,
  `facetCategory`; chips ordered by family: Drums, Percussion & metal, Bass,
  Tonal, Vocal, FX; multi-select), **Key** (`facetKey`; Any / Major / Minor
  segmented control, the 12 roots as multi-select chips, then "Include
  compatible keys" and "Include uncertain"; root `A` adds `A` in Major,
  `Am` in Minor, both in Any; the bar shows one chip per root: `A`, `Am`,
  or `A any`; counts show in Major / Minor only, since in Any a mode-less
  key counts toward both modes), **Tempo** and **Duration**
  (`facetProperty`; min / max fields plus single-select quick-range chips:
  one range each, choosing it sets min/max, choosing it again clears it,
  typing a different bound deselects it; the bar shows one chip for the
  range), **Tags** (`facetTag`; multi-select, most used first),
  **Library** (`facetLibrary`; multi-select, OR within the section; the
  sidebar's library scope still applies on top). Category, Key, Tag and
  Library chips show their count (`caption`, monospaced) and are disabled at
  count 0 unless selected. A footer line shows "n results · m filters" and
  "Clear all". A click applies at once (no Apply). Keyboard: Tab between
  controls, arrows between chips, Space toggles, Esc closes.
- **Results table**: see §4.1.
- **Player header** (ADR-040, v2 ADR-043): always present at the **top** of
  the content column (stable layout), on `surfaceRaised`, hairline below.
  - Transport row (48 pt): play/pause (30 pt circle, `controlAccent` fill,
    white symbol), stop, loop and **AUTO** (when on: `signal` text/icon on
    `signalSoft`; off: plain secondary), a vertical hairline, the file name
    (`title`, middle truncation) and its extension (`caption` tertiary),
    flexible space, then the **Pitch** knob (`knobPitch`) and **Gain** knob
    (`knobGain`), each 24 pt with a `label` caption above its value
    (`readoutValue`): `+0 st`, `−3.0 dB`.
  - **Preview gain** (new, ADR-043): −24 … +6 dB, default 0, 0.5 dB steps
    (Shift 0.1), applied in the preview engine only (never the system
    volume, never the file), remembered across launches; double-click
    resets to 0.
  - Below, 12 pt in from the edges, the **waveform well** (§4.4): ruler,
    lanes, readout strip. With no selection: controls disabled, lanes show a
    flat centre line, readouts show en dashes.
- **Compare mode** (similarity mode only): a compact 72 pt well above the
  selected waveform shows the **similarity target** ("Similar to: name",
  `caption`, with its own small play button; the selected region is drawn
  if the search used one). The large well below shows the browsed row
  ("Selected: name"). Leaving similarity mode removes the target well.
  The similarity bar is one 44 pt row on `surfaceRaised` (target name,
  five compact aspect sliders, icon-only Reset and Close). The drop overlay
  (audio file dragged onto the results) is a `selection` fill with an
  accent hairline border, radius `large`, and a centred caption; no blur.
- **Inspector**: metadata and editing only (no transport, no waveform). A
  header without a card (name `title`, folder path `caption` tertiary, then
  three equal action buttons: Favorite, Similar, Reveal), then **cards**
  (`surfaceGroup`, radius `large`, 10/12 padding, 8 pt apart) with a
  `caption` semibold secondary title: "Analysis" (BPM / key / category /
  kind with overrides; key source line in `caption` tertiary), "Tags"
  (§4.12; `+` button in the card title row), "File" (duration, sample rate,
  bit depth, channels, size, library). Rows are `PropertyRow`s (§4.9).
- **Window**: minimum 900×520. When narrow, hide the inspector first, then
  collapse the sidebar; the table and player header never disappear (the
  waveform lanes shrink to their 96 pt minimum first).

## 4. Components

One implementation each, in `app/Sonar/DesignSystem/Components/`. Use the
system control when one exists; write a component only for the things below.

### 4.1 Results table (AppKit)
- Background `surfaceContent`; **no** alternating rows, no grid lines.
- Header: `label` style, tertiary colour, 24 pt high, hairline below.
- Row 26. Selection: rounded (`small`) tint `selection`, inset 6 pt
  horizontally; text keeps its normal colour (never white-on-accent).
  Unfocused table: `selectionInactive`.
- Columns: status (heart when favourite; the **playing** row shows a
  `speaker.wave.2` symbol in `accent` instead, so selection and playback stay
  distinguishable) · Name (`row`, primary, middle truncation; medium weight
  when selected) · Category (plain `row` text, no dot) · Kind (symbol + text,
  secondary) · BPM (`rowNumeric`, right-aligned) · Key (`row`, left-aligned,
  see "Key / note text" below) · Duration (`rowNumeric`, right-aligned,
  secondary) · Library (secondary). A missing value shows nothing, not "-";
  the one exception is Key, which shows an en dash in tertiary (silence / no
  value, ADR-041).
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
Height 24 (22 inside menus/lists), radius `medium`, `chip` font, 9 pt
horizontal padding, 6 pt gap to its count and `×`. Every chip has a facet hue
(§2.1); there is no un-hued chip any more.
- **Unselected**: hue text on `chipBase` (dark) / the hue's fill at 11 %
  (light); no border.
- **Hover**: background one step lighter (dark: label at 6 % over
  `chipBase`; light: fill at 18 %).
- **Selected / active**: the hue's deep fill (`…Fill`), white text, same
  weight (a chip cloud must not reflow on toggle). An active chip's `×` is
  white at 85 %.
- **Mixed** (tag on only some of the selected files, §4.12): transparent
  background, 1 pt dashed border in the hue, hue text, then "2/3"
  (`caption` monospaced).
- **Disabled**: 38 % opacity, no hover.
- Optional count in `caption` monospaced digits at 72 %; optional trailing
  `×` (own button and action).
Chip clouds wrap with `FlowLayout` (spacing `s6`). One component for filter
chips, active-filter chips and tag chips.

### 4.3 Readout (`Readout`)
Inline label + value, used in the waveform readout strip (§4.4): caption in
`row` secondary, value in `readoutValue` in `signal` (amber; Format stays
primary), 6 pt apart; readouts separated by 16 pt and a 16 pt-tall hairline.
Missing value: an en dash. The Key value is note + superscript cents or key
(`KeyLabel`, §4.1), in `signal`.

### 4.4 Waveform
A `waveWell` box (radius `well`, `waveGrid` inner border) with three parts
(look: `design/reference/sonar-v2/main-*.png`):
- **Ruler** (22 pt): ticks and labels in `wave`. With a BPM: bars.beats
  (`1`, `1.2`, `1.3`, `1.4`, `2` …, sixteenth ticks short); without: seconds
  (`0.0`, `0.5` …, the tick step picked so labels are ≥ 60 pt apart).
  Labels `caption` monospaced. Follows zoom/pan.
- **Lanes**: one lane per channel (stereo = two lanes split by a 2 pt
  `waveGrid` line; mono = one lane), each with a `waveCentre` centre line.
  Bars in `wave`; played part full, unplayed at 0.40. Playhead: 1 pt line in
  label colour from the ruler down, with a small triangle head in the
  ruler. Loading: flat centre line; error: caption text centred.
- **Readout strip** (34 pt, `waveStrip`, `waveGrid` line above): `Readout`s
  BPM · Key · Length · Format (`WAV · 44.1 kHz · Stereo`), then flexible
  space, "Zoom" (`row` secondary) and two 26×22 icon buttons (vertical /
  horizontal zoom) with `wave`-coloured symbols.
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
DAW-style rotary control (look: the hardware reference, flat). A
**coloured face** in its parameter family colour (`knobPitch`, `knobGain`,
…, §2.1), 1 pt dark rim, a faint top highlight arc, a dark pointer line
from 25 % to 82 % of the radius. Around it a 270° track (label colour at
10 %) with the part between the default and the value in the face colour.
Sizes: 24 in bars (value as text beside it, `label` caption above the
value), 44 in panels (caption above, value below). Vertical drag (6 pt per
step, up = more; Shift = fine), double-click resets to the default, arrow
keys / VoiceOver adjust (role adjustable, value with unit). Never paired
with a Reset button; the value text is secondary when at the default.
Colour identifies the parameter family, never the individual knob.

### 4.9 Property row (`PropertyRow`)
Inspector grid row, at least 24 pt high: label in `row` secondary on the left
(`Theme.Size.propertyLabelWidth`, 72), value or editor right-aligned in the
rest. Read-only values are selectable `row` text (monospaced digits for
numbers); missing value: an en dash in tertiary. Rows sit in the inspector cards
(§3) with 2 pt between them. The Analysis "Key" value is the `KeyLabel`
rendering (§4.1); clicking it switches to the text field (Return commits,
losing focus cancels), a trailing revert button clears the override; below it (one sample selected) read-only secondary rows give
"Key source" (`Audio` / `Name` / `Audio + Name` / `You`, then the
confidence word `High` / `Medium` / `Low`, e.g. `Audio · High`), "Name says"
(the filename's key, only when it disagrees with the audio key), "Audio says"
(the audio's key, only when the name outweighed a disagreeing audio key); a
one-shot's note is in the Key row itself. The Key section of the filter panel has "Include uncertain" (low-tier
keys) next to "Include compatible keys". Removable tags are `SonarChip`s in the
`facetTag` hue (§4.2) with a trailing `xmark`.

### 4.10 Status badge (`StatusBadge`)
`caption` text in a status colour (`Theme.Color.ok` / `warning` / `error`) on
the same colour at 16 %, radius `small`, 6 / 2 padding. Online / offline
libraries (Settings), exact / near duplicate groups.

### 4.11 Settings and sheets
Settings tabs are system grouped `Form`s (`.formStyle(.grouped)`), hints in
`caption` secondary, errors in `Theme.Color.error`. The Libraries tab is a
list plus a button row. Sheets (Enter License) are standard sheets with a
`title` heading, a `surfaceGroup` text well with a hairline border, and
Cancel / default-action buttons.

### 4.12 Tags (ADR-043; replaces the T10.9 tag editor sheet)
Look: `design/reference/sonar-v2/tags-*.png`. Every tag chip is a
`facetTag` `SonarChip`; no colour dots in front of tags anywhere.

**Normalisation** (core, ADR-043): a tag is trimmed, inner whitespace runs
collapse to one space, and tags are unique **case-insensitively**
("Club ", "club", "CLUB" are one tag; the stored spelling is the most used
one). Similar tags (typos) are suggested, never merged silently (Manage).

**Three layers, fast to deep:**
1. **Context menu → "Tags ›"** (replaces "Edit Tags…"): a submenu with the 8
   most used tags, each with a tri-state mark for the selected rows (✓ all,
   – some, none), then a separator, **Add Tag…** (⌘T) and **Manage Tags…**.
   Choosing a tag adds it to all selected rows (or removes it when all have
   it); the menu closes; ⌘Z undoes. The Edit menu has the same "Tags"
   submenu.
2. **Tag popover** (⌘T, "Add Tag…", the inspector Tags card `+`): a system
   popover (width 340) anchored to the selected row or the `+` button.
   Header: `tag` symbol in `facetTag`, "Tags" (`body` bold), the file name or
   "n samples" (`caption` tertiary). A token field (focused, accent focus
   ring) with the current tags as chips with `×` (mixed tags in the dashed
   mixed style with "2/3"; clicking a mixed chip adds it to all) and an
   inline field. Below, a list (26 pt rows, check column, name, count right
   in `caption` monospaced): with an empty field the section "Most used"
   with ✓ / – marks; while typing the matches (prefix first, then
   contains), the first highlighted, and as the **last** row "Create tag
   “x”" (with `plus`) when no exact match exists. Changes apply **live**
   (no Save/Cancel; each change is one Undo step). Keys: ↓/↑ move, Return
   toggles the highlighted row (or creates), comma/Tab commits the text,
   ⌫ on an empty field removes the last chip, Esc closes. Footer:
   "Manage Tags…" link (accent) and a `caption` hint "↩ add · esc close".
3. **Manage Tags** sheet (Manage Tags… in the menus and the popover): title
   "Manage Tags", subtitle "n tags · All libraries", a filter field. A
   **similar tags card** (`signalSoft`, shown only when there are any):
   "n similar tags — may be typos", each pair as two chips with their counts,
   the reason (`caption` tertiary: "1 letter", "hyphen", "spelling"), a
   "Merge into “x”" button (the more used spelling wins) and an icon-only
   "Ignore" (`xmark`; the pair is remembered and not suggested again, e.g.
   "dark"/"park"); "Merge All" in the card title; a note that case and spaces merge automatically. Then a list
   of all tags (name, file count right, "unused" in tertiary for 0) with
   multi-select; double-click renames inline; bottom bar `+` / `−` (delete
   asks for confirmation with the file count; Undo), "n selected" hint,
   "Merge…" (2+ selected; pick the surviving name) and "Done" (default).
Inspector Tags card: the file's tags as chips with `×`, the mixed state for
a multi-selection, and the `+` that opens the popover.

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
- `signal` (amber) only for on-state toggles, the waveform readout values
  and the trial badge; never as a fill behind white text, never for a
  selection.
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

1. The implementing agent runs it and looks at the PNGs (Read tool) next to
   the target mockups `design/reference/sonar-v2/<screen>-<light|dark>.png`
   before reporting done; it fixes what it sees.
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
