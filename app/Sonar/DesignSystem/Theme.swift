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
        /// Waveform well.
        static let well: CGFloat = 8
        static let large: CGFloat = 10
    }

    /// Fixed sizes of the content layer.
    enum Size {
        static let rowHeight: CGFloat = 26
        /// Raise of the cents superscript of a note (`KeyLabel`).
        static let superscriptBaseline: CGFloat = 4
        static let headerHeight: CGFloat = 24
        static let barHeight: CGFloat = 44
        /// Transport row of the player header (DESIGN §2.3).
        static let transportRowHeight: CGFloat = 48
        /// Lanes of the selected-waveform well in the player header: default,
        /// minimum and maximum height (user-resizable, DESIGN §2.3). The
        /// ruler and the readout strip come on top.
        static let waveformHeight: CGFloat = 150
        static let waveformMinHeight: CGFloat = 96
        static let waveformMaxHeight: CGFloat = 320
        /// Ruler above the lanes and readout strip below them (DESIGN §4.4).
        static let rulerHeight: CGFloat = 22
        static let readoutStripHeight: CGFloat = 34
        /// Divider between the lanes of a stereo waveform.
        static let laneDivider: CGFloat = 2
        /// Zoom icon buttons in the readout strip.
        static let zoomButtonWidth: CGFloat = 26
        static let zoomButtonHeight: CGFloat = 22
        /// Playhead head (triangle) in the ruler.
        static let playheadHead: CGFloat = 8
        /// Compare mode: similarity-target well height and the caption row
        /// above each well (DESIGN §3).
        static let compareWellHeight: CGFloat = 72
        static let compareLabelHeight: CGFloat = 22
        /// Width of one aspect slider in the similarity bar.
        static let aspectSliderWidth: CGFloat = 48
        /// Height of the draggable split divider under the player header.
        static let splitHandle: CGFloat = 6
        static let transportButton: CGFloat = 28
        /// Round play button of the player header.
        static let playButton: CGFloat = 30
        /// Chip height; `chipHeightCompact` inside menus and lists.
        static let chipHeight: CGFloat = 24
        static let chipHeightCompact: CGFloat = 22
        /// `Knob` face diameters: 24 in bars, 44 in panels.
        static let knobSmall: CGFloat = 24
        static let knobLarge: CGFloat = 44
        /// Value arc of a knob: stroke width and gap outside the face.
        static let knobArc: CGFloat = 2.2
        /// Waveform region: edge line width and drag-handle size (DESIGN §4.4).
        static let regionEdge: CGFloat = 1
        static let regionHandleWidth: CGFloat = 6
        static let regionHandleHeight: CGFloat = 24
        /// Width of the floating readout + menu on a region.
        static let regionBarWidth: CGFloat = 200
        /// Symbol point size of `EmptyStateView`.
        static let emptyStateSymbol: CGFloat = 28
        /// Label column of `PropertyRow` (inspector grids).
        static let propertyLabelWidth: CGFloat = 72
        /// Vertical padding of an inspector card (horizontal is `Space.s12`).
        static let cardPaddingVertical: CGFloat = 10
        /// Maximum text width of `EmptyStateView`.
        static let emptyStateTextWidth: CGFloat = 360
        /// Filter panel (DESIGN §3): popover width, height it scrolls past,
        /// and the min/max number fields of the Tempo / Duration sections.
        static let filterPanelWidth: CGFloat = 480
        static let filterPanelMaxHeight: CGFloat = 640
        static let rangeField: CGFloat = 64
        /// Hue square before a filter-panel section title.
        static let hueSquare: CGFloat = 8
        /// Round close button of the panel header.
        static let panelCloseButton: CGFloat = 24
        /// Tag editor sheet (DESIGN §4.11): width, token-box minimum height,
        /// inline field width, suggestion list height, checkmark column.
        static let tagEditorWidth: CGFloat = 420
        static let tagBoxMinHeight: CGFloat = 56
        static let tagFieldWidth: CGFloat = 96
        static let tagListHeight: CGFloat = 200
        static let tagCheckWidth: CGFloat = 14
        /// Progress bar of the toolbar indexing status.
        static let indexingProgressWidth: CGFloat = 80
    }

    /// Opacity of the unplayed part of the waveform (played = full accent).
    static let waveformUnplayedOpacity: Double = 0.40
    /// Transport buttons: pressed and disabled (matches the chip's 38 %).
    static let pressedOpacity: Double = 0.7
    static let disabledOpacity: Double = 0.38

    /// One physical pixel at the given backing scale (2 = Retina).
    static func hairlineWidth(scale: CGFloat) -> CGFloat { 1 / max(scale, 1) }

    /// Hairline thickness for SwiftUI: 1 physical pixel on Retina displays,
    /// which every Mac that runs macOS 14 has (use `hairlineWidth(scale:)`
    /// when the real scale is known).
    static let hairlineWidth: CGFloat = 0.5
}
