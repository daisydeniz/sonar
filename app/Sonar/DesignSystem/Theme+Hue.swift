import AppKit
import SwiftUI

extension Theme {
    /// Facet hues of the filter panel and the hued `SonarChip` (DESIGN §2.1,
    /// ADR-042): the six category hues plus `property` (Key, Tempo, Duration)
    /// and `tag`. Each has a text colour (light/dark aware) and a deep fill
    /// for the selected state (white text on it, ≥ 4.5:1).
    enum Hue: CaseIterable, Sendable {
        case drum, metal, bass, tonal, vocal, fx, property, tag

        var textAsset: String {
            switch self {
            case .drum: return "CatDrum"
            case .metal: return "CatMetal"
            case .bass: return "CatBass"
            case .tonal: return "CatTonal"
            case .vocal: return "CatVocal"
            case .fx: return "CatFX"
            case .property: return "FacetProperty"
            case .tag: return "FacetTag"
            }
        }

        var fillAsset: String { textAsset + "Fill" }

        /// Text / icon colour on a neutral surface.
        var color: SwiftUI.Color { SwiftUI.Color(textAsset) }
        /// Selected-state background (same in light and dark).
        var fill: SwiftUI.Color { SwiftUI.Color(fillAsset) }
        /// Unselected chip background: the hue at 16 %.
        var tint: SwiftUI.Color { color.opacity(0.16) }
        /// Hovered unselected chip background: the hue at 24 %.
        var tintHover: SwiftUI.Color { color.opacity(0.24) }

        var nsFill: NSColor { NS.named(fillAsset) }
    }

    /// Opacity of the count next to a hued chip's title.
    static let chipCountOpacity: Double = 0.7

    /// Hue of a core category string (`nil` = missing category).
    static func categoryHue(_ category: String?) -> Hue? {
        categoryToken(category).map { token in
            switch token {
            case .drum: return .drum
            case .metal: return .metal
            case .bass: return .bass
            case .tonal: return .tonal
            case .vocal: return .vocal
            case .fx: return .fx
            }
        }
    }
}
