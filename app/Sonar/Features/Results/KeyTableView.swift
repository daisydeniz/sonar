import AppKit

/// `NSTableView` that reports Space / Return / Esc instead of beeping;
/// arrows and everything else stay native. It also owns the hover row of the
/// results table (the favourite heart's outline).
final class KeyTableView: NSTableView {
    /// Returns true when the key was handled.
    var onKey: ((TableKey) -> Bool)?
    /// ⌘C: copy the selected file URLs.
    var onCopy: (() -> Void)?

    @objc func copy(_ sender: Any?) { onCopy?() }

    /// True while ⌘A runs: the selection grows but focus must not move.
    private(set) var isSelectingAll = false

    /// Row under the pointer, -1 for none. Recomputed from the real pointer
    /// position on every mouse move and every scroll, so it cannot go stale
    /// when rows scroll under a still pointer (per-row tracking areas miss
    /// `mouseExited` then and left hearts stuck on).
    private(set) var hoveredRow = -1 {
        didSet {
            guard hoveredRow != oldValue else { return }
            hoverView(oldValue)?.isHovered = false
            hoverView(hoveredRow)?.isHovered = true
        }
    }
    private var hoverArea: NSTrackingArea?

    /// AppKit makes the LAST row `selectedRow` after select-all; put the
    /// focus back on the row that had it.
    override func selectAll(_ sender: Any?) {
        let old = selectedRow
        isSelectingAll = true
        super.selectAll(sender)
        if old >= 0 { selectRowIndexes([old], byExtendingSelection: true) }
        isSelectingAll = false
    }

    override func keyDown(with event: NSEvent) {
        if let key = TableKey.from(event), onKey?(key) == true { return }
        super.keyDown(with: event)
    }

    // MARK: Hover

    /// Gives a row view the current hover state when it is (re)used for `row`.
    func applyHover(to rowView: NSTableRowView, row: Int) {
        (rowView as? ResultsRowView)?.isHovered = row == hoveredRow
    }

    private func hoverView(_ row: Int) -> ResultsRowView? {
        guard row >= 0, row < numberOfRows else { return nil }
        return rowView(atRow: row, makeIfNecessary: false) as? ResultsRowView
    }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        guard hoverArea == nil else { return }
        let area = NSTrackingArea(
            rect: .zero, options: [.mouseEnteredAndExited, .mouseMoved, .activeInActiveApp, .inVisibleRect],
            owner: self)
        addTrackingArea(area)
        hoverArea = area
    }

    override func mouseMoved(with event: NSEvent) {
        super.mouseMoved(with: event)
        refreshHover()
    }

    override func mouseEntered(with event: NSEvent) {
        super.mouseEntered(with: event)
        if event.trackingArea === hoverArea { refreshHover() }
    }

    override func mouseExited(with event: NSEvent) {
        super.mouseExited(with: event)
        if event.trackingArea === hoverArea { hoveredRow = -1 }
    }

    override func viewDidMoveToSuperview() {
        super.viewDidMoveToSuperview()
        NotificationCenter.default.removeObserver(self, name: NSView.boundsDidChangeNotification, object: nil)
        guard let clip = superview as? NSClipView else { return }
        clip.postsBoundsChangedNotifications = true
        NotificationCenter.default.addObserver(
            self, selector: #selector(clipBoundsChanged), name: NSView.boundsDidChangeNotification, object: clip)
    }

    @objc private func clipBoundsChanged(_ note: Notification) { refreshHover() }

    override func reloadData() {
        super.reloadData()
        refreshHover()
    }

    /// Hover = the row under the pointer, if the pointer is inside the
    /// visible part of the table while the app is active.
    func refreshHover() {
        guard let window, NSApp.isActive else {
            hoveredRow = -1
            return
        }
        let point = convert(window.mouseLocationOutsideOfEventStream, from: nil)
        hoveredRow = visibleRect.contains(point) ? row(at: point) : -1
    }
}
