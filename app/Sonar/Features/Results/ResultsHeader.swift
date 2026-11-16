import AppKit

/// Header cell in the `label` style (DESIGN §4.1): uppercase, tracked,
/// secondary colour, `Space.s8` insets, sort chevron next to the title.
/// Draws no background and no column separators.
@MainActor
final class ResultsHeaderCell: NSTableHeaderCell {
    /// Column identifier, to find this column's sort direction.
    var columnID = ""

    private static let chevrons: [Bool: NSImage] = [true: tinted("chevron.up"), false: tinted("chevron.down")]

    /// Template symbol drawn in the secondary colour of the drawing appearance.
    private static func tinted(_ name: String) -> NSImage {
        let config = NSImage.SymbolConfiguration(pointSize: Theme.NSFonts.label.pointSize, weight: .bold)
        let symbol = NSImage(systemSymbolName: name, accessibilityDescription: nil)?.withSymbolConfiguration(config)
        let size = symbol?.size ?? .zero
        return NSImage(size: size, flipped: false) { rect in
            symbol?.draw(in: rect)
            NSColor.secondaryLabelColor.set()
            rect.fill(using: .sourceAtop)
            return true
        }
    }

    /// Sort direction to show for `columnID`: only the FIRST descriptor
    /// counts (the core sorts by one key; AppKit keeps older descriptors
    /// behind the newest). nil = no indicator.
    nonisolated static func sortAscending(for columnID: String, in descriptors: [NSSortDescriptor]) -> Bool? {
        guard let first = descriptors.first, first.key == columnID else { return nil }
        return first.ascending
    }

    override func draw(withFrame cellFrame: NSRect, in controlView: NSView) {
        drawInterior(withFrame: cellFrame, in: controlView)
    }

    override func drawInterior(withFrame cellFrame: NSRect, in controlView: NSView) {
        let descriptors = (controlView as? NSTableHeaderView)?.tableView?.sortDescriptors ?? []
        let ascending = Self.sortAscending(for: columnID, in: descriptors)
        let paragraph = NSMutableParagraphStyle()
        paragraph.lineBreakMode = .byTruncatingTail
        let text = NSAttributedString(
            string: stringValue.uppercased(),
            attributes: [
                .font: Theme.NSFonts.label, .kern: Theme.NSFonts.labelKern,
                .foregroundColor: NSColor.secondaryLabelColor, .paragraphStyle: paragraph,
            ])
        let chevron = ascending.flatMap { Self.chevrons[$0] }
        let chevronWidth = chevron.map { $0.size.width + Theme.Space.s4 } ?? 0
        let inner = cellFrame.insetBy(dx: Theme.Space.s8, dy: 0)
        let textSize = text.size()
        let width = min(ceil(textSize.width), max(inner.width - chevronWidth, 0))
        let rightAligned = alignment == .right
        let textX = rightAligned ? inner.maxX - width : inner.minX
        text.draw(in: NSRect(x: textX, y: cellFrame.midY - textSize.height / 2, width: width, height: textSize.height))
        guard let chevron else { return }
        let x = rightAligned ? textX - chevronWidth : textX + width + Theme.Space.s4
        let rect = NSRect(
            x: x, y: cellFrame.midY - chevron.size.height / 2, width: chevron.size.width, height: chevron.size.height)
        chevron.draw(in: rect, from: .zero, operation: .sourceOver, fraction: 1, respectFlipped: true, hints: nil)
    }
}

/// Header strip on the content surface with a hairline below.
final class ResultsHeaderView: NSTableHeaderView {
    override func draw(_ dirtyRect: NSRect) {
        Theme.NS.surfaceContent.setFill()
        bounds.fill()
        super.draw(dirtyRect)
        let line = Theme.hairlineWidth(scale: window?.backingScaleFactor ?? 2)
        Theme.NS.hairline.setFill()
        NSRect(x: bounds.minX, y: isFlipped ? bounds.maxY - line : bounds.minY, width: bounds.width, height: line).fill()
    }
}
