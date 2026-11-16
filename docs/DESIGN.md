# Sonar — Design

How Sonar looks and feels. `CLAUDE.md` describes behaviour of the team,
`PRODUCT.md` what the app does, this file what it looks like. Every UI task
reads this file first and uses **only** the tokens defined here
(`app/Sonar/DesignSystem/`). ADR-039 records the decision.

## 1. Direction

**A precise instrument, native to the Mac.** Sonar is a pro audio tool: dense,
calm, fast to scan, keyboard-first. The reference mix is Sononym's information
density, Ableton's browser discipline and Linear's polish, on a standard macOS
frame.

- **System frame, custom content.** Window chrome (sidebar, toolbar, inspector
  container, menus, sheets, settings) is standard SwiftUI/AppKit, so it gets
  the current macOS look (Liquid Glass on macOS 26+) for free. Sonar's own
  identity lives in the content layer: results table, player dock, waveform,
  chips, meters.
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
| `surfaceRaised` | `#F4F5F7` | `#1E2023` | filter bar, similarity bar, player dock, cards |
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
- `accentSoft` = `accent` at 14 %: active chip / toggle background.
- Category chip/dot background = the category hue at 16 %, text = the hue.

Category → hue goes through `Theme.categoryColor(_:)` only. Unknown or missing
category: no dot, text in tertiary.

### 2.2 Type

SF Pro (system font) only. Four steps plus one label style:

| Token | Size / weight | Use |
|---|---|---|
| `title` | 15 semibold | inspector name, empty-state title, dock title |
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
- **Sizes**: table row 24 · table header 22 · bar height 36 · player dock 104 ·
  waveform in dock fills its height minus 2×8 · icon in rows/bars 12–13 pt
  (`.imageScale(.small)`), transport button 28.
- **Hairlines** are 1 physical pixel (`Theme.hairlineWidth`).

### 2.4 Motion

Almost none. Selection, sort and filter changes are instant. Allowed: system
sidebar/inspector animations; 120 ms ease-out fade for banners and hover
states; the playhead follows audio via display link. No springs, no bounces.

## 3. Layout

```
┌──────────┬───────────────────────────────────────────┬────────────┐
│ Sidebar  │ Filter bar (36)                           │ Inspector  │
│ (system) ├───────────────────────────────────────────┤ (system    │
│          │ Similarity bar (only in similarity mode)  │ container, │
│          ├───────────────────────────────────────────┤ Sonar      │
│          │ Results table (surfaceContent, 24 pt rows)│ content)   │
│          │                                           │            │
│          ├───────────────────────────────────────────┤            │
│          │ Player dock (104): transport · waveform · │            │
│          │ readouts                                  │            │
└──────────┴───────────────────────────────────────────┴────────────┘
```

- **Sidebar**: standard `.sidebar` list. SF Symbols, counts as badges, offline
  libraries dimmed with a warning symbol. "Add…" actions live in the section
  header (a `+` button), not as fake rows.
- **Toolbar**: search field (centre/trailing), indexing status, inspector
  toggle. No custom-drawn toolbar items.
- **Filter bar**: ONE row, `surfaceRaised`, hairline below. Kind as a compact
  segmented control (All / Loops / One-shots); Category, Key, BPM, Duration,
  Tags as popover buttons; Favorites as an icon toggle; result count right,
  `caption`, monospaced digits. A popover button with a selection turns into
  an active chip (`accentSoft` background, accent text, shows the selection
  summary, has a clear `×`). Facet lists with counts live inside the popovers.
- **Results table**: see §4.1.
- **Player dock**: always present under the table (stable layout), on
  `surfaceRaised` with a hairline above. Left: transport (play/stop, loop,
  auto-play) and file name. Centre: waveform in a `surfaceWell`, full
  remaining width. Right: readouts (BPM, key, duration) and pitch. With no
  selection: controls disabled, well shows a flat centre line.
- **Inspector**: metadata and editing only (no transport, no waveform): header
  (name `title`, folder path `caption`), actions (favorite, find similar,
  reveal), then sections "Analysis" (BPM / key / category / kind with
  overrides), "Tags", "File". Sections use a `label` heading and a two-column
  grid (label secondary left, value right); not a grouped settings `Form`.
- **Window**: minimum 900×520. When narrow, hide the inspector first, then
  collapse the sidebar; the table and dock never disappear.

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
  · Kind (symbol + text, secondary) · BPM, Key, Duration (`rowNumeric`,
  right-aligned; missing value = nothing, not "-") · Library (secondary).
- Hidden files: whole row at tertiary. Loading rows: a short neutral skeleton
  bar in the Name column, not the word "Loading…".

### 4.2 Chip (`SonarChip`)
Height 22, radius `small`, `row` font, 8 pt horizontal padding. States:
default (hairline border, secondary text) · hover (`surfaceWell`) · active
(`accentSoft`, accent text, no border) · disabled (40 % opacity). Optional
count in `caption` monospaced digits; optional leading category dot.

### 4.3 Readout (`Readout`)
A value with a caption above it: caption in `label`, value in 15 pt medium
monospaced digits. Used in the player dock (BPM, KEY, LENGTH, PITCH). Missing
value: an en dash in tertiary.

### 4.4 Waveform
Bars in `accent` on `surfaceWell`, radius `medium`. Played part full accent,
unplayed part accent at 45 %. Playhead: 1 pt line in label colour. Loading:
flat centre line; error: caption text centred in the well.

### 4.5 Section header (`SectionLabel`)
`label` style text, secondary colour, 16 above / 6 below.

### 4.6 Empty and error states
Centred: SF Symbol 28 pt in tertiary, `title`, one `body` line in secondary,
at most one primary action. Same component everywhere (`EmptyStateView`).

### 4.7 Banner
One line on `surfaceRaised`, leading status symbol in the system status
colour, `row` text, trailing action or dismiss. Never a tinted full-width
background.

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

A token-rule violation or a broken light/dark state is BLOCKING for UI tasks.
