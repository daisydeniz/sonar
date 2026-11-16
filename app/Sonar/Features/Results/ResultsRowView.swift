import AppKit

/// Row of the results table: rounded tinted selection (DESIGN §4.1) and
/// hover tracking for the favourite heart. No layer effects, no timers.
final class ResultsRowView: NSTableRowView {
    static let identifier = NSUserInterfaceItemIdentifier("sonar.row")

    /// TEST / SNAPSHOT ONLY: forces the focused (true) or inactive (false)
    /// selection look. The offscreen snapshot window is never key, so
    /// `scripts/ui-snapshots.sh` sets this instead of stealing keyboard
    /// focus. nil (always, in the app) = follow the real focus state.
    @MainActor static var emphasisOverride: Bool?

    /// The pointer is over this row (the favourite cell shows its outline).
    private(set) var isHovered = false {
        didSet { if isHovered != oldValue { applyHover() } }
    }

    override var isEmphasized: Bool {
        get { Self.emphasisOverride ?? super.isEmphasized }
        set { super.isEmphasized = newValue }
    }

    /// Text keeps its normal colour on a selected row (never white).
    override var interiorBackgroundStyle: NSView.BackgroundStyle { .normal }

    override func drawSelection(in dirtyRect: NSRect) {
        (isEmphasized ? Theme.NS.selection : Theme.NS.selectionInactive).setFill()
        let rect = bounds.insetBy(dx: Theme.Space.s4, dy: 0)
        NSBezierPath(roundedRect: rect, xRadius: Theme.Radius.small, yRadius: Theme.Radius.small).fill()
    }

    override func prepareForReuse() {
        super.prepareForReuse()
        isHovered = false
    }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        for area in trackingAreas { removeTrackingArea(area) }
        addTrackingArea(
            NSTrackingArea(
                rect: .zero, options: [.mouseEnteredAndExited, .activeInActiveApp, .inVisibleRect], owner: self))
    }

    override func mouseEntered(with event: NSEvent) { isHovered = true }
    override func mouseExited(with event: NSEvent) { isHovered = false }

    override func didAddSubview(_ subview: NSView) {
        super.didAddSubview(subview)
        (subview as? FavoriteCellView)?.setRowHovered(isHovered)
    }

    private func applyHover() {
        for case let cell as FavoriteCellView in subviews { cell.setRowHovered(isHovered) }
    }
}
