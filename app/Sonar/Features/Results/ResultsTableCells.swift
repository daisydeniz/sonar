import AppKit

/// Base of the text cells of the results table (DESIGN §4.1): one label
/// vertically centred with `Space.s8` insets. Subclasses add an accessory
/// (dot, symbol, skeleton bar) and fill themselves in `configure`.
@MainActor
class ResultCellView: NSTableCellView {
    let label = NSTextField(labelWithString: "")
    /// The label's leading constraint; accessory cells re-pin it.
    private(set) var labelLeading: NSLayoutConstraint!

    init(
        identifier: NSUserInterfaceItemIdentifier, font: NSFont, alignment: NSTextAlignment = .left,
        truncation: NSLineBreakMode = .byTruncatingTail
    ) {
        super.init(frame: .zero)
        self.identifier = identifier
        label.font = font
        label.alignment = alignment
        label.lineBreakMode = truncation
        label.translatesAutoresizingMaskIntoConstraints = false
        addSubview(label)
        textField = label
        labelLeading = label.leadingAnchor.constraint(equalTo: leadingAnchor, constant: Theme.Space.s8)
        NSLayoutConstraint.activate([
            labelLeading,
            label.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -Theme.Space.s8),
            label.centerYAnchor.constraint(equalTo: centerYAnchor),
        ])
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("init(coder:) is not used") }

    /// Colour of the label for a visible (not hidden) file.
    var baseColor: NSColor { .labelColor }

    /// Fills the cell from a loaded row. Subclasses call super last.
    func configure(_ file: FileRow, libraryName: String?) {
        label.textColor = file.hidden ? .tertiaryLabelColor : baseColor
        label.setAccessibilityLabel(nil)
    }

    /// The row's page is not loaded yet: nothing but (in Name) a skeleton bar.
    func showLoading() {
        label.stringValue = ""
        label.setAccessibilityLabel("Loading")
    }

    /// Puts `view` (size `size`) in front of the label, vertically centred.
    func addLeadingAccessory(_ view: NSView, size: CGFloat) {
        view.translatesAutoresizingMaskIntoConstraints = false
        addSubview(view)
        labelLeading.isActive = false
        labelLeading = label.leadingAnchor.constraint(equalTo: view.trailingAnchor, constant: Theme.Space.s6)
        NSLayoutConstraint.activate([
            view.leadingAnchor.constraint(equalTo: leadingAnchor, constant: Theme.Space.s8),
            view.centerYAnchor.constraint(equalTo: centerYAnchor),
            view.widthAnchor.constraint(equalToConstant: size),
            view.heightAnchor.constraint(equalToConstant: size),
            labelLeading,
        ])
    }
}

/// A rounded neutral bar standing in for a name that is not loaded yet.
final class SkeletonBarView: NSView {
    override func draw(_ dirtyRect: NSRect) {
        Theme.NS.surfaceWell.setFill()
        NSBezierPath(roundedRect: bounds, xRadius: Theme.Radius.small, yRadius: Theme.Radius.small).fill()
    }
}

/// A filled circle in one colour (category dot).
final class DotView: NSView {
    var color: NSColor? { didSet { needsDisplay = true } }
    override func draw(_ dirtyRect: NSRect) {
        guard let color else { return }
        color.setFill()
        NSBezierPath(ovalIn: bounds).fill()
    }
}

/// Name column: primary text, middle truncation, skeleton bar while loading.
final class NameCellView: ResultCellView {
    private let skeleton = SkeletonBarView()

    init(identifier: NSUserInterfaceItemIdentifier) {
        super.init(identifier: identifier, font: Theme.NSFonts.row, truncation: .byTruncatingMiddle)
        skeleton.translatesAutoresizingMaskIntoConstraints = false
        skeleton.isHidden = true
        addSubview(skeleton)
        NSLayoutConstraint.activate([
            skeleton.leadingAnchor.constraint(equalTo: leadingAnchor, constant: Theme.Space.s8),
            skeleton.centerYAnchor.constraint(equalTo: centerYAnchor),
            skeleton.heightAnchor.constraint(equalToConstant: Theme.Space.s8),
            skeleton.widthAnchor.constraint(equalTo: widthAnchor, multiplier: 0.4),
        ])
    }

    override func configure(_ file: FileRow, libraryName: String?) {
        skeleton.isHidden = true
        label.stringValue = file.name
        super.configure(file, libraryName: libraryName)
    }

    override func showLoading() {
        super.showLoading()
        skeleton.isHidden = false
    }
}

/// Category column: dot in the category hue + capitalised name.
final class CategoryCellView: ResultCellView {
    private let dot = DotView()

    init(identifier: NSUserInterfaceItemIdentifier) {
        super.init(identifier: identifier, font: Theme.NSFonts.row)
        addLeadingAccessory(dot, size: Theme.Size.categoryDot)
    }

    override func configure(_ file: FileRow, libraryName: String?) {
        label.stringValue = Format.categoryTitle(file.category)
        dot.color = Theme.categoryNSColor(file.category)
        dot.alphaValue = file.hidden ? 0.4 : 1
        super.configure(file, libraryName: libraryName)
    }

    override func showLoading() {
        super.showLoading()
        dot.color = nil
    }
}

/// Kind column: SF Symbol + "Loop" / "One-shot" in secondary colour.
final class KindCellView: ResultCellView {
    private let icon = NSImageView()
    private static var symbols: [String: NSImage] = [:]

    init(identifier: NSUserInterfaceItemIdentifier) {
        super.init(identifier: identifier, font: Theme.NSFonts.row)
        icon.imageScaling = .scaleProportionallyDown
        addLeadingAccessory(icon, size: Theme.Space.s12)
    }

    override var baseColor: NSColor { .secondaryLabelColor }

    /// Symbol images are created once per name.
    private static func symbol(_ name: String) -> NSImage? {
        if let image = symbols[name] { return image }
        let config = NSImage.SymbolConfiguration(pointSize: Theme.NSFonts.caption.pointSize, weight: .regular)
        let image = NSImage(systemSymbolName: name, accessibilityDescription: nil)?.withSymbolConfiguration(config)
        symbols[name] = image
        return image
    }

    override func configure(_ file: FileRow, libraryName: String?) {
        label.stringValue = Format.kindTitle(file.kind)
        icon.image = Format.kindSymbol(file.kind).flatMap(Self.symbol)
        icon.contentTintColor = file.hidden ? .tertiaryLabelColor : .secondaryLabelColor
        super.configure(file, libraryName: libraryName)
    }

    override func showLoading() {
        super.showLoading()
        icon.image = nil
    }
}

/// BPM, Key, Duration and Library: one label, the text comes from `text`.
final class ValueCellView: ResultCellView {
    /// Which value of the row the cell shows.
    enum Value { case bpm, key, duration, library }
    private let value: Value

    init(identifier: NSUserInterfaceItemIdentifier, value: Value) {
        self.value = value
        let numeric = value == .bpm || value == .duration
        super.init(
            identifier: identifier, font: numeric ? Theme.NSFonts.rowNumeric : Theme.NSFonts.row,
            alignment: numeric ? .right : .left)
    }

    override var baseColor: NSColor { value == .library ? .secondaryLabelColor : .labelColor }

    override func configure(_ file: FileRow, libraryName: String?) {
        switch value {
        case .bpm: label.stringValue = Format.cellBPM(file.bpm)
        case .key: label.stringValue = Format.cellText(file.key)
        case .duration: label.stringValue = Format.cellDuration(ms: file.durationMs)
        case .library: label.stringValue = Format.cellText(libraryName)
        }
        super.configure(file, libraryName: libraryName)
    }
}
