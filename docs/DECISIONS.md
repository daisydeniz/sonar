# Architecture Decision Log

Append new decisions at the bottom. Never delete; supersede instead.
Format: `ADR-NNN — title` · status · context · decision · consequences.

## ADR-001 — macOS first, Windows in ~1 year
Accepted. Founder develops on a MacBook Pro M4 (48 GB). Best UX on Mac is the
goal. Windows is likely later, so platform-neutral logic must be portable.

## ADR-002 — Rust core + Swift (SwiftUI/AppKit) UI, bridged with UniFFI
Accepted. Rust holds all product logic (scan, DSP, index, search) and is
reused on Windows. Swift gives native UX, AVAudioEngine preview, native drag &
drop. Rejected: Tauri (webview list/preview limits for "best UX"), pure Swift
(Windows rewrite of the valuable part), JUCE app (slow UI dev, C++ risk).

## ADR-003 — Direct distribution, not Mac App Store
Accepted. App Store sandbox blocks v2 plugin hosting (plugins need ~/Library,
licenses, iLok). Notarized DMG + Paddle or Lemon Squeezy (merchant of record).
Hardened runtime, no sandbox.

## ADR-004 — Permissive dependencies only
Accepted. Closed-source commercial product → MIT/Apache/BSD/ISC/Zlib/MPL-2.0
(file-level) only. No GPL/AGPL (aubio, Essentia, libKeyFinder). Enforced with
`cargo deny`. ML model weights must also allow commercial redistribution;
record model + license in this log before shipping.
Amendment (T0.2): also allowed `Apache-2.0 WITH LLVM-exception` and
`Unicode-3.0` (Unicode data tables, e.g. `unicode-ident`). Allow list lives in
`core/deny.toml`; any new license needs an amendment here first.

## ADR-005 — MVP scope: Sononym parity + natural-language search
Accepted. Preset search moves to v2. See docs/PRODUCT.md.

## ADR-006 — Local CLAP embeddings via ONNX Runtime
Accepted (model choice pending, see M5). Audio+text joint embeddings for
natural-language search and zero-shot categories. Run via `ort` with the
CoreML execution provider on macOS (Neural Engine/GPU), CPU fallback.

## ADR-007 — v2 plugin host = C++/JUCE sidecar process
Accepted for v2. One host for Mac (AU + VST3) and later Windows (VST3).
Built for arm64 and x86_64 (Intel-only plugins under Rosetta). Separate
process for crash isolation. VST2-only and 32-bit plugins unsupported.

## ADR-008 — Quality measured, not asserted
Accepted. Every analysis/search change runs `sonar eval` against synthetic
fixtures and (when present) the golden set; regressions block merge.

## ADR-009 — Scanner file identity and walk policy (T1.2)
Accepted. Identity = `size` + `mtime` (ns) + `quick_hash`, where
`quick_hash` = XXH3-64 (`twox-hash`, MIT) over the size as LE u64 + first
64 KiB + last 64 KiB (whole file if ≤ 128 KiB), stored bit-cast as INTEGER.
Unchanged size+mtime → the file is not opened. Moves/renames are matched by
(size, quick_hash) within one library, so ids and user data survive.
Changing the recipe invalidates every stored hash (needs a migration that
rehashes). Symlinks are **not** followed (no loops, no double-indexing);
dot-files/dirs and AppleDouble `._*` are skipped. Missing files become
`status='missing'` (rows and user data kept); an offline root or an
unreadable folder never marks rows missing. Revisit symlink support if users
keep packs behind links (follow with a visited-inode set).

## ADR-010 — Decode policy (T1.3)
Accepted. `sonar_core::decode` uses symphonia 0.6.1 (MPL-2.0; wav, aiff,
pcm, adpcm, flac, mp3, ogg/vorbis, isomp4/aac/alac, id3v2) and rubato 5.0.1
`Fft` resampler (MIT/Apache; chunk 4096, 4 sub-chunks, delay trimmed and
tail flushed → `ceil(n·22050/src)` samples, ±1-sample alignment). Analysis
input = mean-of-channels mono f32 at 22.05 kHz, capped at 30 s (+ an edge
margin of max(50 ms, 2 resampler blocks) decoded past the cap). Duration is
**counted**: decoded frames in the window + packet durations past it (raw
ticks, converted once); counted < declared → `partial`. Policy: a packet
the codec rejects → silence of equal length + `partial` (> 32 in a row
stops); malformed container mid-stream → `partial`; real I/O error (not EOF)
→ `Error::Io` (offline drive must not be stored as a damaged file); nothing
decoded + error → error; valid header with 0 frames → Ok, empty; 0-byte file
→ error. Hard caps: 1 kHz–768 kHz, 1–32 ch, ≤ 2^18 frames/packet, ≤ 2^23
packets, ≤ 64 Mi source frames buffered. Every call runs under
`catch_unwind` (symphonia-format-riff 0.6.1 has a u16 overflow panic on a
crafted WAV header in debug builds).
