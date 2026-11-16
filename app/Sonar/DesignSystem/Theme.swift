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
        static let playerDockHeight: CGFloat = 104
        static let transportButton: CGFloat = 28
        static let chipHeight: CGFloat = 22
        /// Symbol point size of `EmptyStateView`.
        static let emptyStateSymbol: CGFloat = 28
        /// Maximum text width of `EmptyStateView`.
        static let emptyStateTextWidth: CGFloat = 360
    }

    /// One physical pixel at the given backing scale (2 = Retina).
    static func hairlineWidth(scale: CGFloat) -> CGFloat { 1 / max(scale, 1) }

    /// Hairline thickness for SwiftUI: 1 physical pixel on Retina displays,
    /// which every Mac that runs macOS 14 has (use `hairlineWidth(scale:)`
    /// when the real scale is known).
    static let hairlineWidth: CGFloat = 0.5
}
