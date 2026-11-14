import AppKit

/// Heart cell of the results table: filled when the file is a favourite, a
/// faint outline otherwise. A click toggles it (the controller is the target).
final class FavoriteCellView: NSTableCellView {
    private let button = NSButton()

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

    /// Shows `row`'s state; nil (page still loading) hides the heart.
    func show(_ row: FileRow?) {
        button.isHidden = row == nil
        guard let row else { return }
        isFavorite = row.favorite
        let label = row.favorite ? "Unfavorite" : "Favorite"
        button.image = NSImage(systemSymbolName: row.favorite ? "heart.fill" : "heart", accessibilityDescription: label)
        button.contentTintColor = row.favorite ? .systemPink : .tertiaryLabelColor
        button.toolTip = label
        button.setAccessibilityLabel(label)
    }
}
