import AppKit
import SwiftUI

extension Theme {
    /// Colour tokens as SwiftUI `Color`. Values live in
    /// `Assets.xcassets/Colors` (light + dark); derived ones are computed.
    /// Text uses the system semantic colours, status the system colours.
    enum Color {
        /// Content accent: waveform, playhead, selection tint, active chip,
        /// accent-coloured text and icons. Bright in dark mode: never put
        /// text on it (use `controlAccent` for filled controls).
        static let accent = SwiftUI.Color("AccentContent")
        /// Fill of system controls (the app's `AccentColor` asset: default
        /// and prominent buttons, menu highlight, sliders, focus ring).
        /// White text on it passes 4.5:1 in both appearances.
        static let controlAccent = SwiftUI.Color("AccentColor")
        /// Results table and main content background.
        static let surfaceContent = SwiftUI.Color("SurfaceContent")
        /// Filter bar, similarity bar, player header, cards.
        static let surfaceRaised = SwiftUI.Color("SurfaceRaised")
        /// Waveform well, inset areas, meter tracks.
        static let surfaceWell = SwiftUI.Color("SurfaceWell")
        /// 1 px separators and borders.
        static let hairline = SwiftUI.Color("Hairline")
        /// Filled favourite heart.
        static let favorite = SwiftUI.Color("Favorite")
        static let catDrum = SwiftUI.Color("CatDrum")
        static let catMetal = SwiftUI.Color("CatMetal")
        static let catBass = SwiftUI.Color("CatBass")
        static let catTonal = SwiftUI.Color("CatTonal")
        static let catVocal = SwiftUI.Color("CatVocal")
        static let catFX = SwiftUI.Color("CatFX")

        /// Status: error (system red).
        static let error = SwiftUI.Color(nsColor: .systemRed)
        /// Status: warning (system orange).
        static let warning = SwiftUI.Color(nsColor: .systemOrange)
        /// Status: ok (system green).
        static let ok = SwiftUI.Color(nsColor: .systemGreen)

        /// Section group inside the filter panel: a semantic fill that keeps
        /// the popover's glass visible (not an opaque surface).
        static var panelGroup: SwiftUI.Color { SwiftUI.Color(nsColor: .quinarySystemFill) }
        /// Focused selected row: accent at 16 % (light) / 24 % (dark).
        static var selection: SwiftUI.Color { SwiftUI.Color(nsColor: Theme.NS.selection) }
        /// Selected row while the table is not focused: label colour at 8 %.
        static var selectionInactive: SwiftUI.Color { SwiftUI.Color(nsColor: Theme.NS.selectionInactive) }
        /// Active chip / toggle background: accent at 14 %.
        static var accentSoft: SwiftUI.Color { SwiftUI.Color(nsColor: Theme.NS.accentSoft) }
    }

    /// The same tokens as `NSColor`, for AppKit code (table, waveform).
    /// Computed (not stored): `NSColor` is not `Sendable`.
    enum NS {
        static var accent: NSColor { named("AccentContent") }
        static var controlAccent: NSColor { named("AccentColor") }
        static var surfaceContent: NSColor { named("SurfaceContent") }
        static var surfaceRaised: NSColor { named("SurfaceRaised") }
        static var surfaceWell: NSColor { named("SurfaceWell") }
        static var hairline: NSColor { named("Hairline") }
        static var favorite: NSColor { named("Favorite") }
        static var catDrum: NSColor { named("CatDrum") }
        static var catMetal: NSColor { named("CatMetal") }
        static var catBass: NSColor { named("CatBass") }
        static var catTonal: NSColor { named("CatTonal") }
        static var catVocal: NSColor { named("CatVocal") }
        static var catFX: NSColor { named("CatFX") }

        static var error: NSColor { .systemRed }
        static var warning: NSColor { .systemOrange }
        static var ok: NSColor { .systemGreen }

        /// Focused selected row: accent at 16 % (light) / 24 % (dark).
        static var selection: NSColor {
            derived { isDark in (NS.accent, isDark ? 0.24 : 0.16) }
        }
        /// Selected row, table not focused: label colour at 8 %.
        static var selectionInactive: NSColor {
            derived { _ in (NSColor.labelColor, 0.08) }
        }
        /// Active chip / toggle background: accent at 14 %.
        static var accentSoft: NSColor {
            derived { _ in (NS.accent, 0.14) }
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

    /// Category hue for a core category string, or nil (no dot) when the
    /// category is missing. Unknown non-nil values count as FX.
    static func categoryColor(_ category: String?) -> SwiftUI.Color? {
        categoryToken(category).map { SwiftUI.Color($0.assetName) }
    }

    /// `NSColor` twin of `categoryColor(_:)`.
    static func categoryNSColor(_ category: String?) -> NSColor? {
        categoryToken(category).map { NS.named($0.assetName) }
    }

    enum CategoryToken {
        case drum, metal, bass, tonal, vocal, fx
        var assetName: String {
            switch self {
            case .drum: return "CatDrum"
            case .metal: return "CatMetal"
            case .bass: return "CatBass"
            case .tonal: return "CatTonal"
            case .vocal: return "CatVocal"
            case .fx: return "CatFX"
            }
        }
    }

    static func categoryToken(_ category: String?) -> CategoryToken? {
        guard let category else { return nil }
        switch category.lowercased() {
        case "kick", "snare", "clap", "tom", "drums": return .drum
        case "hat", "cymbal", "perc": return .metal
        case "bass": return .bass
        case "lead", "pad", "keys", "pluck", "synth": return .tonal
        case "vocal": return .vocal
        default: return .fx
        }
    }
}
