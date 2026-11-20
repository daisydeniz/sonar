import AppKit

/// Row of the results table: rounded tinted selection (DESIGN §4.1) and the
/// hover state for the favourite heart. The hover state is set by
/// `KeyTableView` (one table-wide tracking area + scroll observation), never
/// by per-row tracking areas: those miss `mouseExited` while scrolling reuses
/// rows, which left hearts stuck on. No layer effects, no timers.
final class ResultsRowView: NSTableRowView {
    static let identifier = NSUserInterfaceItemIdentifier("sonar.row")

    /// TEST / SNAPSHOT ONLY: forces the focused (true) or inactive (false)
    /// selection look. The offscreen snapshot window is never key, so
    /// `scripts/ui-snapshots.sh` sets this instead of stealing keyboard
    /// focus. nil (always, in the app) = follow the real focus state.
    @MainActor static var emphasisOverride: Bool?

    /// The pointer is over this row (the favourite cell shows its outline).
    var isHovered = false {
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
        let rect = bounds.insetBy(dx: Theme.Space.s6, dy: 0)
        NSBezierPath(roundedRect: rect, xRadius: Theme.Radius.small, yRadius: Theme.Radius.small).fill()
    }

    override func prepareForReuse() {
        super.prepareForReuse()
        isHovered = false
    }

    override func didAddSubview(_ subview: NSView) {
        super.didAddSubview(subview)
        (subview as? FavoriteCellView)?.setRowHovered(isHovered)
    }

    private func applyHover() {
        for case let cell as FavoriteCellView in subviews { cell.setRowHovered(isHovered) }
    }
}
