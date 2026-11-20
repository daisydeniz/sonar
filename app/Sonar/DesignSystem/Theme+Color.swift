import AppKit
import SwiftUI

extension Theme {
    /// Colour tokens as SwiftUI `Color` (DESIGN §2.1, ADR-043). Values live in
    /// `Assets.xcassets/Colors` (light + dark); derived ones are computed.
    /// Text uses the system semantic colours, status the system colours.
    enum Color {
        /// Content accent: playhead head, selection tint, Filters button,
        /// links. Bright in dark mode: never put text on it (use
        /// `controlAccent` for filled controls).
        static let accent = SwiftUI.Color("AccentContent")
        /// The app's `AccentColor` asset: default/prominent buttons, play
        /// button, menu highlight, focus ring. White on it passes 4.5:1.
        static let controlAccent = SwiftUI.Color("AccentColor")
        /// Amber: "on" mode toggles, readout values, trial badge.
        static let signal = SwiftUI.Color("Signal")
        /// Results table and main content background.
        static let surfaceContent = SwiftUI.Color("SurfaceContent")
        /// Player header, filter bar, similarity bar.
        static let surfaceRaised = SwiftUI.Color("SurfaceRaised")
        /// Cards: inspector, filter panel sections, tag list well.
        static let surfaceGroup = SwiftUI.Color("SurfaceGroup")
        /// Unselected chip background in dark (light uses the hue at 11 %).
        static let chipBase = SwiftUI.Color("ChipBase")
        /// Waveform well, its readout strip, and the waveform itself.
        static let waveWell = SwiftUI.Color("WaveWell")
        static let waveStrip = SwiftUI.Color("WaveStrip")
        static let wave = SwiftUI.Color("Wave")
        /// 1 px separators and borders.
        static let hairline = SwiftUI.Color("Hairline")
        /// Filled favourite heart.
        static let favorite = SwiftUI.Color("Favorite")

        /// Knob family faces (same in both appearances except neutral).
        static let knobPitch = SwiftUI.Color("KnobPitch")
        static let knobGain = SwiftUI.Color("KnobGain")
        static let knobFilter = SwiftUI.Color("KnobFilter")
        static let knobTime = SwiftUI.Color("KnobTime")
        static let knobNeutral = SwiftUI.Color("KnobNeutral")

        /// Status: error (system red).
        static let error = SwiftUI.Color(nsColor: .systemRed)
        /// Status: warning (system orange).
        static let warning = SwiftUI.Color(nsColor: .systemOrange)
        /// Status: ok (system green).
        static let ok = SwiftUI.Color(nsColor: .systemGreen)

        /// Focused selected row: `accent` at 12 % (light) / 20 % (dark).
        static var selection: SwiftUI.Color { SwiftUI.Color(nsColor: Theme.NS.selection) }
        /// Selected row while the table is not focused: label colour at 8 %.
        static var selectionInactive: SwiftUI.Color { SwiftUI.Color(nsColor: Theme.NS.selectionInactive) }
        /// Background of an "on" toggle and the trial badge.
        static var signalSoft: SwiftUI.Color { SwiftUI.Color(nsColor: Theme.NS.signalSoft) }
        /// Waveform lane divider and well border.
        static var waveGrid: SwiftUI.Color { SwiftUI.Color(nsColor: Theme.NS.waveGrid) }
        /// Waveform lane centre line.
        static var waveCentre: SwiftUI.Color { SwiftUI.Color(nsColor: Theme.NS.waveCentre) }
    }

    /// The same tokens as `NSColor`, for AppKit code (table, waveform).
    /// Computed (not stored): `NSColor` is not `Sendable`.
    enum NS {
        static var accent: NSColor { named("AccentContent") }
        static var controlAccent: NSColor { named("AccentColor") }
        static var signal: NSColor { named("Signal") }
        static var surfaceContent: NSColor { named("SurfaceContent") }
        static var surfaceRaised: NSColor { named("SurfaceRaised") }
        static var surfaceGroup: NSColor { named("SurfaceGroup") }
        static var chipBase: NSColor { named("ChipBase") }
        static var waveWell: NSColor { named("WaveWell") }
        static var waveStrip: NSColor { named("WaveStrip") }
        static var wave: NSColor { named("Wave") }
        static var hairline: NSColor { named("Hairline") }
        static var favorite: NSColor { named("Favorite") }

        static var error: NSColor { .systemRed }
        static var warning: NSColor { .systemOrange }
        static var ok: NSColor { .systemGreen }

        /// Focused selected row: accent at 12 % (light) / 20 % (dark).
        static var selection: NSColor {
            derived { isDark in (NS.accent, isDark ? 0.20 : 0.12) }
        }
        /// Selected row, table not focused: label colour at 8 %.
        static var selectionInactive: NSColor {
            derived { _ in (NSColor.labelColor, 0.08) }
        }
        /// "On" toggle / trial badge background: signal at 18 % / 16 %.
        static var signalSoft: NSColor {
            derived { isDark in (NS.signal, isDark ? 0.16 : 0.18) }
        }
        /// Waveform grid: wave at 10 % (light) / 14 % (dark).
        static var waveGrid: NSColor {
            derived { isDark in (NS.wave, isDark ? 0.14 : 0.10) }
        }
        /// Waveform centre line: wave at 28 % / 32 %.
        static var waveCentre: NSColor {
            derived { isDark in (NS.wave, isDark ? 0.32 : 0.28) }
        }

        static func named(_ name: String) -> NSColor {
            NSColor(named: NSColor.Name(name), bundle: .main) ?? .magenta  // magenta = missing asset
        }

        /// A dynamic colour: `make` picks base colour and alpha per appearance.
        private static func derived(_ make: @escaping (_ isDark: Bool) -> (NSColor, CGFloat)) -> NSColor {
            NSColor(name: nil) { appearance in
                let isDark = appearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua
                let (base, alpha) = make(isDark)
                var out = base
                appearance.performAsCurrentDrawingAppearance {
                    out = (base.usingColorSpace(.sRGB) ?? base).withAlphaComponent(alpha)
                }
                return out
            }
        }
    }
}
