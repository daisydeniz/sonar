import AppKit

/// Heart cell of the results table: a filled heart in the favourite colour
/// when the file is a favourite; otherwise an outline that is visible only
/// while the pointer is over the row. The button stays clickable either way
/// (hidden = alpha 0, not `isHidden`). The controller is the click target.
final class FavoriteCellView: NSTableCellView {
    private let button = NSButton()
    private static var images: [Bool: NSImage] = [:]
    private var rowHovered = false
    private var loaded = false

    init(identifier: NSUserInterfaceItemIdentifier, target: AnyObject, action: Selector) {
        super.init(frame: .zero)
        self.identifier = identifier
        button.isBordered = false
        button.imagePosition = .imageOnly
        button.imageScaling = .scaleProportionallyDown
        button.target = target
        button.action = action
        button.translatesAutoresizingMaskIntoConstraints = false
        addSubview(button)
        NSLayoutConstraint.activate([
            button.centerXAnchor.constraint(equalTo: centerXAnchor),
            button.centerYAnchor.constraint(equalTo: centerYAnchor),
            button.widthAnchor.constraint(equalToConstant: 18),
            button.heightAnchor.constraint(equalToConstant: 18),
        ])
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("init(coder:) is not used") }

    /// Whether the cell currently shows a filled heart (tests).
    private(set) var isFavorite = false

    /// Whether the outline / heart is currently visible (tests).
    var isHeartVisible: Bool { !button.isHidden && button.alphaValue > 0 }

    private static func image(filled: Bool) -> NSImage? {
        if let image = images[filled] { return image }
        let config = NSImage.SymbolConfiguration(pointSize: Theme.NSFonts.row.pointSize, weight: .regular)
        let image = NSImage(systemSymbolName: filled ? "heart.fill" : "heart", accessibilityDescription: nil)?
            .withSymbolConfiguration(config)
        images[filled] = image
        return image
    }

    /// Shows `row`'s state; nil (page still loading) hides the heart.
    func show(_ row: FileRow?) {
        loaded = row != nil
        button.isHidden = row == nil
        guard let row else { return }
        isFavorite = row.favorite
        let label = row.favorite ? "Unfavorite" : "Favorite"
        button.image = Self.image(filled: row.favorite)
        button.contentTintColor = row.favorite ? Theme.NS.favorite : .tertiaryLabelColor
        button.toolTip = label
        button.setAccessibilityLabel(label)
        rowHovered = (superview as? ResultsRowView)?.isHovered ?? rowHovered
        updateVisibility()
    }

    /// The pointer entered / left the row (called by `ResultsRowView`).
    func setRowHovered(_ hovered: Bool) {
        rowHovered = hovered
        updateVisibility()
    }

    private func updateVisibility() {
        button.alphaValue = loaded && (isFavorite || rowHovered) ? 1 : 0
    }
}
