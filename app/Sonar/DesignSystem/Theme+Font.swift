import AppKit
import SwiftUI

extension Theme {
    /// Type scale (DESIGN §2.2), SF Pro only. Numbers that line up use the
    /// `*Numeric` variants (monospaced digits).
    enum Font {
        /// 15 semibold: inspector name, empty-state title, dock title.
        static let title = SwiftUI.Font.system(size: 15, weight: .semibold)
        /// 13 regular: default controls, inspector values, sidebar.
        static let body = SwiftUI.Font.system(size: 13)
        /// 12 medium: every chip (same weight selected or not).
        static let chip = SwiftUI.Font.system(size: 12, weight: .medium)
        /// 12 regular: table cells, bar controls.
        static let row = SwiftUI.Font.system(size: 12)
        /// `row` with monospaced digits.
        static let rowNumeric = SwiftUI.Font.system(size: 12).monospacedDigit()
        /// 11 regular: secondary info, counts, hints.
        static let caption = SwiftUI.Font.system(size: 11)
        /// `caption` with monospaced digits.
        static let captionNumeric = SwiftUI.Font.system(size: 11).monospacedDigit()
        /// 10 semibold; use with `sonarLabelStyle()` (uppercase, +0.6 tracking).
        static let label = SwiftUI.Font.system(size: 10, weight: .semibold)
        /// 13 medium, monospaced digits: readout and knob values.
        static let readoutValue = SwiftUI.Font.system(size: 13, weight: .medium).monospacedDigit()
        /// 9 regular, monospaced digits: the cents superscript of a note (`KeyLabel`).
        static let superscript = SwiftUI.Font.system(size: 9).monospacedDigit()
    }

    /// `NSFont` twins for the AppKit table. Computed: `NSFont` is not `Sendable`.
    enum NSFonts {
        /// Tracking of the label style, as an `NSAttributedString.Key.kern` value.
        static let labelKern: CGFloat = 0.6
        static var row: NSFont { .systemFont(ofSize: 12) }
        static var rowNumeric: NSFont { .monospacedDigitSystemFont(ofSize: 12, weight: .regular) }
        static var label: NSFont { .systemFont(ofSize: 10, weight: .semibold) }
        static var caption: NSFont { .systemFont(ofSize: 11) }
        /// Cents superscript of a note (`KeyLabel`).
        static var superscript: NSFont { .monospacedDigitSystemFont(ofSize: 9, weight: .regular) }
    }
}

extension View {
    /// The label style: 10 semibold, uppercase, +0.6 tracking (section
    /// labels, table header, readout captions). Colour is the caller's.
    func sonarLabelStyle() -> some View {
        font(Theme.Font.label).tracking(0.6).textCase(.uppercase)
    }
}
