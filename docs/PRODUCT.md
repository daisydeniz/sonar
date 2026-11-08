# Sonar — Product Definition (code name)

A macOS app that indexes very large local sample libraries and makes them
instantly searchable — by text, by filters, by **sound similarity**, and by
**natural-language description**. Reference product: Sononym. Goal: be the
sample browser producers praise for speed, search quality and feel.

## Users
Music producers with large local libraries (10k–1M+ files) across internal and
external drives. Primary DAW of the founder: Ableton Live; the app must work
with every DAW (drag & drop is the integration).

## Positioning
- Cheaper entry than Sononym, better search, native Mac feel.
- 100% local / offline. No account, no cloud, no telemetry by default.
- Sold directly (notarized DMG + Paddle/Lemon Squeezy), not via Mac App Store.

## Non-negotiable quality bars ("praise targets")
| Metric | Target |
|---|---|
| Search-as-you-type latency (500k files) | p95 < 50 ms |
| Preview start latency after selection | < 10 ms perceived (pre-buffered) |
| Initial scan (metadata only) | > 2,000 files/s on M-series SSD |
| Full analysis throughput (DSP + CLAP) | > 100 files/s on M4, in background, UI never blocks |
| Result list | 1M rows scroll at 120 fps |
| Category accuracy (golden set) | ≥ 90 % top-1 for core drum/instrument classes |
| BPM accuracy on loops (±1 BPM, octave errors counted separately) | ≥ 90 % |
| Key accuracy (MIREX weighted) | ≥ 80 % |
| Similarity P@10 (golden similarity sets) | ≥ 0.7 |
| Crash-free | Corrupt/odd files never crash the app; they are flagged |

## MVP scope (v1)
Sononym-parity features plus one differentiator (natural-language search).

1. **Libraries**: add folders; background scan; incremental rescan; file
   watching; external drives stay browsable while offline (metadata cached).
2. **Analysis**: duration, format, loudness, loop vs one-shot, BPM (loops only;
   filename prior), key, pitch/note, category (kick, snare, clap, hat, cymbal,
   perc, tom, bass, lead, pad, keys, pluck, vocal, fx, texture, …).
3. **Text search**: filename + path tokens, synonyms (bd/kick, hh/hat…),
   exclusion (`-vinyl`), smart tokenization (`120bpm`, `Cmin`, `F#`).
4. **Filters & sort**: category, type, BPM range, key (+ compatible keys),
   duration, library, tags, favorites; sort by any column.
5. **Similarity search**: "find similar" from a library item or a dropped
   external file; aspect weights: Overall, Spectrum, Timbre, Pitch, Amplitude.
6. **Natural-language search (differentiator)**: "dark metallic kick",
   "airy female vocal chop" via local CLAP embeddings, fused with text search.
7. **Preview**: waveform, auto-play on select, keyboard navigation, pitch
   control, loop playback, volume normalization.
8. **Organization**: favorites, collections, tags, user overrides for
   BPM/key/category (never overwritten by re-analysis), hide from results.
9. **Drag & drop** to any DAW / Finder.
10. **Duplicates**: exact (hash) and near-duplicate (perceptual) finder.

## v1.x (after MVP)
Record-to-search (mic/synth input), 2D similarity map view, preview synced to
project tempo, kit builder (export Ableton Drum Rack), Ableton `.als` project
map (where is a sample used / unused samples).

## v2
**Preset search** for popular synths (Serum, Sylenth1, Massive first): index
preset metadata (fxp/fxb, vstpreset, aupreset, NKS) and render each preset
through a headless plugin host (C++/JUCE sidecar, AU + VST3, arm64 + x86_64
under Rosetta) with a standard MIDI phrase, then search presets by sound like
samples. VST2-only and 32-bit plugins are out of scope. Presets that cannot be
rendered are still searchable by metadata.

## Later
Windows version (~1 year): reuse the Rust core, new UI.

## Explicitly out of scope for v1
Cloud sync, accounts, sample store, editing audio, plugin (in-DAW) version,
preset search, Windows.
