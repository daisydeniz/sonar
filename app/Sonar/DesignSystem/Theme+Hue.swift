import AppKit
import SwiftUI

extension Theme {
    /// Facet hues (DESIGN §2.1, ADR-043): one per filter section, used by
    /// every chip that shows a value of that section. Text colour is
    /// light/dark aware; the deep fill is for the selected state (white text
    /// on it, ≥ 4.5:1, token test).
    enum Hue: CaseIterable, Sendable {
        case category, key, property, tag, library

        var textAsset: String {
            switch self {
            case .category: return "FacetCategory"
            case .key: return "FacetKey"
            case .property: return "FacetProperty"
            case .tag: return "FacetTag"
            case .library: return "FacetLibrary"
            }
        }

        var fillAsset: String { textAsset + "Fill" }

        /// Text / icon colour on a neutral surface.
        var color: SwiftUI.Color { SwiftUI.Color(textAsset) }
        /// Selected-state background (same in light and dark).
        var fill: SwiftUI.Color { SwiftUI.Color(fillAsset) }
        /// Unselected chip background in light: the fill at 11 %.
        var tint: SwiftUI.Color { fill.opacity(0.11) }
        /// Hovered unselected chip background in light: the fill at 18 %.
        var tintHover: SwiftUI.Color { fill.opacity(0.18) }

        var nsColor: NSColor { NS.named(textAsset) }
        var nsFill: NSColor { NS.named(fillAsset) }
    }

    /// Opacity of the count next to a chip's title.
    static let chipCountOpacity: Double = 0.72
    /// Opacity of a disabled chip.
    static let chipDisabledOpacity: Double = 0.38
    /// Opacity of the `×` on a selected chip.
    static let chipClearOpacity: Double = 0.85
    /// Hover overlay on a dark chip: label colour at 6 %.
    static let chipHoverOverlayOpacity: Double = 0.06

    /// Knob parts (DESIGN §4.8): pointer black at 55 %, track label at 10 %,
    /// rim black at 35 %, top highlight white at 35 %, neutral value arc.
    static let knobPointerOpacity: Double = 0.55
    static let knobTrackOpacity: Double = 0.10
    static let knobRimOpacity: Double = 0.35
    static let knobHighlightOpacity: Double = 0.35
    static let knobNeutralArcOpacity: Double = 0.55
}
