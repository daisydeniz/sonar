import AppKit

/// Status cell of the results table. The playing row shows a `speaker.wave.2`
/// symbol in `accent` instead of the heart (DESIGN §4.1). Otherwise: a filled heart in the favourite colour
/// when the file is a favourite; otherwise an outline that is visible only
/// while the pointer is over the row. The button stays clickable either way
/// (hidden = alpha 0, not `isHidden`). The controller is the click target.
final class FavoriteCellView: NSTableCellView {
    private let button = NSButton()
    private static var images: [String: NSImage] = [:]
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

    /// Whether the cell currently shows the playing speaker (tests).
    private(set) var isPlaying = false

    /// Whether the outline / heart is currently visible (tests).
    var isHeartVisible: Bool { !button.isHidden && button.alphaValue > 0 }

    private static func image(_ name: String) -> NSImage? {
        if let image = images[name] { return image }
        let config = NSImage.SymbolConfiguration(pointSize: Theme.NSFonts.row.pointSize, weight: .regular)
        let image = NSImage(systemSymbolName: name, accessibilityDescription: nil)?
            .withSymbolConfiguration(config)
        images[name] = image
        return image
    }

    /// Shows `row`'s state; nil (page still loading) hides the heart.
    /// `playing`: this row is the one being auditioned.
    func show(_ row: FileRow?, playing: Bool = false) {
        loaded = row != nil
        button.isHidden = row == nil
        guard let row else { return }
        isFavorite = row.favorite
        isPlaying = playing
        let label = row.favorite ? "Unfavorite" : "Favorite"
        if playing {
            button.image = Self.image("speaker.wave.2")
            button.contentTintColor = Theme.NS.accent
        } else {
            button.image = Self.image(row.favorite ? "heart.fill" : "heart")
            button.contentTintColor = row.favorite ? Theme.NS.favorite : .tertiaryLabelColor
        }
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
        button.alphaValue = loaded && (isPlaying || isFavorite || rowHovered) ? 1 : 0
    }
}
