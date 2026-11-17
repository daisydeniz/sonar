import CoreGraphics

/// Design tokens of Sonar (docs/DESIGN.md §2). Feature code uses only these:
/// no literal colours, paddings, radii or font sizes.
enum Theme {
    /// Spacing scale in points (DESIGN §2.3). Nothing else is allowed.
    enum Space {
        static let s2: CGFloat = 2
        static let s4: CGFloat = 4
        static let s6: CGFloat = 6
        static let s8: CGFloat = 8
        static let s12: CGFloat = 12
        static let s16: CGFloat = 16
        static let s24: CGFloat = 24
        static let s32: CGFloat = 32
    }

    /// Corner radii: chips/badges/row selection, controls/wells, cards/overlays.
    enum Radius {
        static let small: CGFloat = 4
        static let medium: CGFloat = 6
        static let large: CGFloat = 10
    }

    /// Fixed sizes of the content layer.
    enum Size {
        static let rowHeight: CGFloat = 24
        static let headerHeight: CGFloat = 22
        static let barHeight: CGFloat = 36
        /// Selected-waveform well in the player header: default, minimum and
        /// maximum height (user-resizable, DESIGN §2.3).
        static let waveformHeight: CGFloat = 180
        static let waveformMinHeight: CGFloat = 120
        static let waveformMaxHeight: CGFloat = 360
        /// Compare mode: similarity-target well height and the caption row
        /// above each well (DESIGN §3).
        static let compareWellHeight: CGFloat = 72
        static let compareLabelHeight: CGFloat = 22
        /// Width of one aspect slider in the similarity bar.
        static let aspectSliderWidth: CGFloat = 48
        /// Height of the draggable split divider under the player header.
        static let splitHandle: CGFloat = 6
        static let transportButton: CGFloat = 28
        static let chipHeight: CGFloat = 22
        /// Diameter of `Knob` and the width of its arc.
        static let knob: CGFloat = 26
        static let knobStroke: CGFloat = 2.5
        /// Waveform region: edge line width and drag-handle size (DESIGN §4.4).
        static let regionEdge: CGFloat = 1
        static let regionHandleWidth: CGFloat = 6
        static let regionHandleHeight: CGFloat = 24
        /// Width of the floating readout + menu on a region.
        static let regionBarWidth: CGFloat = 200
        /// Diameter of the category dot in table rows.
        static let categoryDot: CGFloat = 6
        /// Symbol point size of `EmptyStateView`.
        static let emptyStateSymbol: CGFloat = 28
        /// Label column of `PropertyRow` (inspector grids).
        static let propertyLabelWidth: CGFloat = 72
        /// Maximum text width of `EmptyStateView`.
        static let emptyStateTextWidth: CGFloat = 360
        /// Filter-bar popovers: content width, tallest facet list, and the
        /// min/max number fields of range filters.
        static let popoverWidth: CGFloat = 240
        static let popoverListMaxHeight: CGFloat = 240
        static let rangeField: CGFloat = 64
        /// Progress bar of the toolbar indexing status.
        static let indexingProgressWidth: CGFloat = 100
    }

    /// Opacity of the unplayed part of the waveform (played = full accent).
    static let waveformUnplayedOpacity: Double = 0.45

    /// One physical pixel at the given backing scale (2 = Retina).
    static func hairlineWidth(scale: CGFloat) -> CGFloat { 1 / max(scale, 1) }

    /// Hairline thickness for SwiftUI: 1 physical pixel on Retina displays,
    /// which every Mac that runs macOS 14 has (use `hairlineWidth(scale:)`
    /// when the real scale is known).
    static let hairlineWidth: CGFloat = 0.5
}
