import AppKit

/// `NSTableView` that reports Space / Return / Esc instead of beeping;
/// arrows and everything else stay native.
final class KeyTableView: NSTableView {
    /// Returns true when the key was handled.
    var onKey: ((TableKey) -> Bool)?

    override func keyDown(with event: NSEvent) {
        if let key = TableKey.from(event), onKey?(key) == true { return }
        super.keyDown(with: event)
    }
}

/// AppKit side of the results table: a view-based, fixed-row-height
/// `NSTableView` over a `ResultsPager` (ARCHITECTURE §8). Cells are reused;
/// rows whose page is not loaded yet show a placeholder and are filled in
/// when the pager reports the page. Sorting is delegated to the core through
/// `onSort` (header click); nothing is sorted here.
@MainActor
final class ResultsTableController: NSObject, NSTableViewDataSource, NSTableViewDelegate, NSMenuDelegate {
    /// Table column: identifier, header title, width, and the core sort key.
    struct Column {
        let id: String
        let title: String
        let width: CGFloat
        let sortKey: SortKey
        let monospaced: Bool
    }

    static let columns: [Column] = [
        Column(id: "name", title: "Name", width: 260, sortKey: .name, monospaced: false),
        Column(id: "category", title: "Category", width: 90, sortKey: .category, monospaced: false),
        Column(id: "kind", title: "Kind", width: 70, sortKey: .kind, monospaced: false),
        Column(id: "bpm", title: "BPM", width: 56, sortKey: .bpm, monospaced: true),
        Column(id: "key", title: "Key", width: 56, sortKey: .key, monospaced: false),
        Column(id: "duration", title: "Duration", width: 80, sortKey: .duration, monospaced: true),
        Column(id: "library", title: "Library", width: 140, sortKey: .library, monospaced: false),
    ]

    static let rowHeight: CGFloat = 22

    let scrollView = NSScrollView()
    let tableView = KeyTableView()
    let pager: ResultsPager

    /// Selection changed (nil = none, or the selected row is still loading).
    var onSelect: ((FileRow?) -> Void)?
    /// Header click: the core sort key and direction.
    var onSort: ((SortKey, Bool) -> Void)?
    /// Space / Return / Esc in the table (model-level routing).
    var onKey: ((TableKey) -> Bool)? {
        get { tableView.onKey }
        set { tableView.onKey = newValue }
    }
    /// Context menu "Find Similar" on a row.
    var onFindSimilar: ((FileRow) -> Void)?
    /// Library names for the Library column.
    var libraryNames: [Int64: String] = [:] {
        didSet { if libraryNames != oldValue { reloadVisibleRows() } }
    }

    private var selectedID: Int64?
    private var suppressSelection = false

    init(pager: ResultsPager) {
        self.pager = pager
        super.init()
        for spec in Self.columns {
            let column = NSTableColumn(identifier: NSUserInterfaceItemIdentifier(spec.id))
            column.title = spec.title
            column.width = spec.width
            column.minWidth = 40
            column.sortDescriptorPrototype = NSSortDescriptor(key: spec.id, ascending: true)
            tableView.addTableColumn(column)
        }
        tableView.rowHeight = Self.rowHeight
        tableView.usesAutomaticRowHeights = false
        tableView.style = .fullWidth
        tableView.allowsMultipleSelection = false
        tableView.allowsColumnReordering = true
        tableView.usesAlternatingRowBackgroundColors = true
        tableView.dataSource = self
        tableView.delegate = self
        tableView.setAccessibilityLabel("Search results")
        let menu = NSMenu()
        menu.delegate = self
        tableView.menu = menu
        scrollView.documentView = tableView
        scrollView.hasVerticalScroller = true
        scrollView.hasHorizontalScroller = true
        pager.onChange = { [weak self] in self?.pagerChanged($0) }
    }

    /// Moves keyboard focus into the table and selects the first row if
    /// nothing is selected (selection goes through `onSelect`).
    @discardableResult
    func focusTable() -> Bool {
        guard pager.count > 0 else { return false }
        tableView.window?.makeFirstResponder(tableView)
        if tableView.selectedRow < 0 {
            tableView.selectRowIndexes(IndexSet(integer: 0), byExtendingSelection: false)
        }
        return true
    }

    // MARK: Pager changes

    private func pagerChanged(_ change: ResultsPager.Change) {
        switch change {
        case .reset(let preservePosition):
            tableView.reloadData()
            if !preservePosition, pager.count > 0 { tableView.scrollRowToVisible(0) }
            restoreSelection()
        case .countChanged:
            tableView.noteNumberOfRowsChanged()
        case .pagesLoaded(let range):
            let visible = tableView.rows(in: tableView.visibleRect)
            let lo = max(range.lowerBound, visible.location)
            let hi = min(range.upperBound, visible.location + visible.length)
            if lo < hi {
                tableView.reloadData(
                    forRowIndexes: IndexSet(integersIn: lo..<hi),
                    columnIndexes: IndexSet(integersIn: 0..<tableView.numberOfColumns))
            }
            let selected = tableView.selectedRow
            if selectedID == nil, range.contains(selected), let row = pager.peek(selected) {
                selectedID = row.fileId
                onSelect?(row)
            }
        }
    }

    private func reloadVisibleRows() {
        let visible = tableView.rows(in: tableView.visibleRect)
        guard visible.length > 0 else { return }
        tableView.reloadData(
            forRowIndexes: IndexSet(integersIn: visible.location..<visible.location + visible.length),
            columnIndexes: IndexSet(integersIn: 0..<tableView.numberOfColumns))
    }

    /// Keeps the selected file selected after a reset when its row is cached;
    /// otherwise clears the table highlight (the model keeps its snapshot).
    private func restoreSelection() {
        suppressSelection = true
        defer { suppressSelection = false }
        if let id = selectedID, let index = pager.index(ofFileId: id) {
            tableView.selectRowIndexes(IndexSet(integer: index), byExtendingSelection: false)
        } else {
            tableView.deselectAll(nil)
        }
    }

    // MARK: Context menu

    func menuNeedsUpdate(_ menu: NSMenu) {
        menu.removeAllItems()
        guard tableView.clickedRow >= 0, pager.peek(tableView.clickedRow) != nil else { return }
        let item = NSMenuItem(title: "Find Similar", action: #selector(findSimilarClicked), keyEquivalent: "")
        item.target = self
        menu.addItem(item)
    }

    @objc private func findSimilarClicked() {
        let row = tableView.clickedRow
        if row >= 0, let file = pager.peek(row) { onFindSimilar?(file) }
    }

    // MARK: NSTableViewDataSource

    func numberOfRows(in tableView: NSTableView) -> Int { pager.count }

    func tableView(_ tableView: NSTableView, sortDescriptorsDidChange old: [NSSortDescriptor]) {
        guard let d = tableView.sortDescriptors.first, let key = d.key,
            let spec = Self.columns.first(where: { $0.id == key })
        else { return }
        onSort?(spec.sortKey, !d.ascending)
    }

    // MARK: NSTableViewDelegate

    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        guard let tableColumn else { return nil }
        let id = tableColumn.identifier
        let cell = (tableView.makeView(withIdentifier: id, owner: nil) as? NSTableCellView)
            ?? makeCell(identifier: id)
        let field = cell.textField
        if let file = pager.row(at: row) {
            field?.stringValue = text(for: file, column: id.rawValue)
            field?.textColor = .labelColor
        } else {
            field?.stringValue = id.rawValue == "name" ? "Loading…" : ""
            field?.textColor = .tertiaryLabelColor
        }
        return cell
    }

    func tableViewSelectionDidChange(_ notification: Notification) {
        guard !suppressSelection else { return }
        let row = tableView.selectedRow >= 0 ? pager.peek(tableView.selectedRow) : nil
        selectedID = row?.fileId
        onSelect?(row)
    }

    // MARK: Cells

    private func makeCell(identifier: NSUserInterfaceItemIdentifier) -> NSTableCellView {
        let field = NSTextField(labelWithString: "")
        field.lineBreakMode = .byTruncatingTail
        field.translatesAutoresizingMaskIntoConstraints = false
        let monospaced = Self.columns.first { $0.id == identifier.rawValue }?.monospaced ?? false
        field.font = monospaced
            ? .monospacedDigitSystemFont(ofSize: NSFont.systemFontSize, weight: .regular)
            : .systemFont(ofSize: NSFont.systemFontSize)
        let cell = NSTableCellView()
        cell.identifier = identifier
        cell.textField = field
        cell.addSubview(field)
        NSLayoutConstraint.activate([
            field.leadingAnchor.constraint(equalTo: cell.leadingAnchor, constant: 4),
            field.trailingAnchor.constraint(equalTo: cell.trailingAnchor, constant: -4),
            field.centerYAnchor.constraint(equalTo: cell.centerYAnchor),
        ])
        return cell
    }

    private func text(for row: FileRow, column: String) -> String {
        switch column {
        case "name": return row.name
        case "category": return Format.text(row.category)
        case "kind": return Format.text(row.kind)
        case "bpm": return Format.bpm(row.bpm)
        case "key": return Format.text(row.key)
        case "duration": return Format.duration(ms: row.durationMs)
        case "library": return Format.text(libraryNames[row.libraryId])
        default: return ""
        }
    }
}
