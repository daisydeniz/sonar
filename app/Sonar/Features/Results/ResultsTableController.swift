import AppKit

/// Box for a menu entry's closure (`representedObject` needs a class).
final class MenuAction: NSObject {
    let run: @MainActor () -> Void
    init(run: @escaping @MainActor () -> Void) { self.run = run }
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
        /// nil: the column cannot be sorted.
        let sortKey: SortKey?
        let monospaced: Bool
    }

    /// The heart column (click toggles the row's favourite).
    static let favoriteColumnID = "favorite"

    static let columns: [Column] = [
        Column(id: favoriteColumnID, title: "", width: 26, sortKey: nil, monospaced: false),
        Column(id: "name", title: "Name", width: 260, sortKey: .name, monospaced: false),
        Column(id: "category", title: "Category", width: 90, sortKey: .category, monospaced: false),
        Column(id: "kind", title: "Kind", width: 96, sortKey: .kind, monospaced: false),
        Column(id: "bpm", title: "BPM", width: 56, sortKey: .bpm, monospaced: true),
        Column(id: "key", title: "Key", width: 56, sortKey: .key, monospaced: false),
        Column(id: "duration", title: "Duration", width: 80, sortKey: .duration, monospaced: true),
        Column(id: "library", title: "Library", width: 140, sortKey: .library, monospaced: false),
    ]

    static let rowHeight = Theme.Size.rowHeight
    /// Drag source masks: copy only, never move/delete the user's samples.
    static let externalDragMask: NSDragOperation = [.copy, .generic]
    static let localDragMask: NSDragOperation = .copy

    let scrollView = NSScrollView()
    let tableView = KeyTableView()
    let pager: ResultsPager

    /// The focused file changed (nil = none, or the focused row is still
    /// loading). Focus = `NSTableView.selectedRow`, i.e. the most recently
    /// clicked / arrowed row of a multi-selection.
    var onSelect: ((FileRow?) -> Void)?
    /// The whole selection changed: loaded rows only, in table order.
    var onSelectionChange: (([Int64]) -> Void)?
    /// Header click: the core sort key and direction.
    var onSort: ((SortKey, Bool) -> Void)?
    /// Space / Return / Esc in the table (model-level routing).
    var onKey: ((TableKey) -> Bool)? {
        get { tableView.onKey }
        set { tableView.onKey = newValue }
    }
    /// Context menu "Find Similar" on a row.
    var onFindSimilar: ((FileRow) -> Void)?
    /// The heart of a row was clicked.
    var onToggleFavorite: ((FileRow) -> Void)?
    /// Organize items for the right-clicked rows (the model decides).
    var menuEntries: (([FileRow]) -> [RowMenuEntry])?
    /// Library names for the Library column.
    var libraryNames: [Int64: String] = [:] {
        didSet { if libraryNames != oldValue { reloadVisibleRows() } }
    }

    /// The file whose preview is playing (speaker symbol in the status column).
    var playingFileID: Int64? {
        didSet { if playingFileID != oldValue { reloadStatusColumn() } }
    }

    /// The focused file id (what `onSelect` last reported).
    private(set) var selectedID: Int64?
    /// Selected loaded file ids in table order (ids, not rows, so a pager
    /// reset can re-highlight them).
    private(set) var selectedIDs: [Int64] = []
    private var suppressSelection = false
    private var suppressSort = false

    init(pager: ResultsPager) {
        self.pager = pager
        super.init()
        for spec in Self.columns {
            let column = NSTableColumn(identifier: NSUserInterfaceItemIdentifier(spec.id))
            let header = ResultsHeaderCell(textCell: spec.title)
            header.columnID = spec.id
            header.alignment = spec.monospaced ? .right : .left
            column.headerCell = header
            column.width = spec.width
            column.minWidth = 40
            if spec.id == "key" {
                // Never narrower than a detuned note (`A#4⁻⁴⁵`) plus the cell insets.
                let keyMin = KeyLabel.widestWidth() + 2 * Theme.Space.s8
                column.minWidth = keyMin
                column.width = max(spec.width, keyMin)
            }
            if spec.id == Self.favoriteColumnID {
                column.minWidth = spec.width
                column.maxWidth = spec.width
                column.headerToolTip = "Favorite"
            }
            if spec.sortKey != nil {
                column.sortDescriptorPrototype = NSSortDescriptor(key: spec.id, ascending: true)
            }
            tableView.addTableColumn(column)
        }
        tableView.rowHeight = Self.rowHeight
        tableView.usesAutomaticRowHeights = false
        tableView.style = .fullWidth
        tableView.allowsMultipleSelection = true
        tableView.allowsColumnReordering = true
        tableView.usesAlternatingRowBackgroundColors = false
        tableView.gridStyleMask = []
        tableView.intercellSpacing = .zero
        tableView.backgroundColor = Theme.NS.surfaceContent
        tableView.focusRingType = .none
        tableView.headerView = ResultsHeaderView(
            frame: NSRect(x: 0, y: 0, width: 0, height: Theme.Size.headerHeight))
        tableView.dataSource = self
        tableView.delegate = self
        tableView.setAccessibilityLabel("Search results")
        // Never move or delete the user's samples: copy only (DAWs and
        // Finder accept .copy / .generic).
        tableView.setDraggingSourceOperationMask(Self.externalDragMask, forLocal: false)
        tableView.setDraggingSourceOperationMask(Self.localDragMask, forLocal: true)
        tableView.onCopy = { [weak self] in self?.copySelectedURLs() }
        let menu = NSMenu()
        menu.delegate = self
        tableView.menu = menu
        scrollView.documentView = tableView
        scrollView.drawsBackground = true
        scrollView.backgroundColor = Theme.NS.surfaceContent
        scrollView.borderType = .noBorder
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

    /// Makes the header indicator match the model's sort (it changes
    /// without a header click when a similarity search starts). No `onSort`.
    func showSort(_ key: SortKey, descending: Bool) {
        let column = key == .relevance ? nil : Self.columns.first { $0.sortKey == key }
        let current = tableView.sortDescriptors.first
        if current?.key == column?.id, column == nil || current?.ascending == !descending { return }
        suppressSort = true
        defer { suppressSort = false }
        tableView.sortDescriptors = column.map { [NSSortDescriptor(key: $0.id, ascending: !descending)] } ?? []
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
            let ids = tableView.selectedRowIndexes.compactMap { pager.peek($0)?.fileId }
            if ids != selectedIDs {
                selectedIDs = ids
                onSelectionChange?(ids)
            }
            if selectedID == nil, range.contains(selected), let row = pager.peek(selected) {
                selectedID = row.fileId
                onSelect?(row)
            }
        case .rowsChanged(let rows):
            // In-place edit (favourite): redraw those rows only; scroll
            // position and selection are untouched.
            tableView.reloadData(
                forRowIndexes: rows.filteredIndexSet { $0 < tableView.numberOfRows },
                columnIndexes: IndexSet(integersIn: 0..<tableView.numberOfColumns))
        }
    }

    private func reloadStatusColumn() {
        let visible = tableView.rows(in: tableView.visibleRect)
        let column = tableView.column(withIdentifier: NSUserInterfaceItemIdentifier(Self.favoriteColumnID))
        guard visible.length > 0, column >= 0 else { return }
        tableView.reloadData(
            forRowIndexes: IndexSet(integersIn: visible.location..<visible.location + visible.length),
            columnIndexes: IndexSet(integer: column))
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
        let focus = selectedID.flatMap { pager.index(ofFileId: $0) }
        if focus == nil { selectedID = nil }  // the focused file left the results
        let present = Set(selectedIDs.compactMap { pager.index(ofFileId: $0) } + [focus].compactMap { $0 })
        selectedIDs = present.sorted().compactMap { pager.peek($0)?.fileId }
        tableView.deselectAll(nil)
        // Select the focused row last so it stays NSTableView's selectedRow.
        tableView.selectRowIndexes(IndexSet(present.filter { $0 != focus }), byExtendingSelection: false)
        if let focus { tableView.selectRowIndexes(IndexSet(integer: focus), byExtendingSelection: true) }
        onSelectionChange?(selectedIDs)
    }

    // MARK: Context menu

    func menuNeedsUpdate(_ menu: NSMenu) {
        menu.removeAllItems()
        guard tableView.clickedRow >= 0, pager.peek(tableView.clickedRow) != nil else { return }
        let item = NSMenuItem(title: "Find Similar", action: #selector(findSimilarClicked), keyEquivalent: "")
        item.target = self
        menu.addItem(item)
        // Right-clicking inside the selection acts on all of it, else on that row.
        let clicked = tableView.clickedRow
        let ids = tableView.isRowSelected(clicked) ? selectedIDs : [pager.peek(clicked)?.fileId].compactMap { $0 }
        let rows = ids.compactMap { pager.cachedRow(fileId: $0) }
        let entries = menuEntries?(rows) ?? []
        if !entries.isEmpty { menu.addItem(.separator()) }
        for entry in entries { menu.addItem(makeItem(entry)) }
    }

    private func makeItem(_ entry: RowMenuEntry) -> NSMenuItem {
        if entry.isSeparator { return .separator() }
        let item = NSMenuItem(title: entry.title, action: nil, keyEquivalent: "")
        if !entry.children.isEmpty {
            let sub = NSMenu(title: entry.title)
            for child in entry.children { sub.addItem(makeItem(child)) }
            item.submenu = sub
        } else if let action = entry.action {
            item.action = #selector(menuEntryClicked(_:))
            item.target = self
            item.representedObject = MenuAction(run: action)
        }
        return item
    }

    @objc private func menuEntryClicked(_ sender: NSMenuItem) {
        (sender.representedObject as? MenuAction)?.run()
    }

    /// Heart click: toggles that row only (the selection does not change).
    @objc private func favoriteClicked(_ sender: NSView) {
        let row = tableView.row(for: sender)
        if row >= 0, let file = pager.peek(row) { onToggleFavorite?(file) }
    }

    @objc private func findSimilarClicked() {
        let row = tableView.clickedRow
        if row >= 0, let file = pager.peek(row) { onFindSimilar?(file) }
    }

    // MARK: NSTableViewDataSource

    func numberOfRows(in tableView: NSTableView) -> Int { pager.count }

    func tableView(_ tableView: NSTableView, sortDescriptorsDidChange old: [NSSortDescriptor]) {
        guard !suppressSort, let d = tableView.sortDescriptors.first, let key = d.key,
            let sortKey = Self.columns.first(where: { $0.id == key })?.sortKey
        else { return }
        onSort?(sortKey, !d.ascending)
    }

    // MARK: NSTableViewDelegate

    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        guard let tableColumn else { return nil }
        let id = tableColumn.identifier
        if id.rawValue == Self.favoriteColumnID {
            let cell = (tableView.makeView(withIdentifier: id, owner: nil) as? FavoriteCellView)
                ?? FavoriteCellView(identifier: id, target: self, action: #selector(favoriteClicked(_:)))
            let file = pager.row(at: row)
            cell.show(file, playing: file != nil && file?.fileId == playingFileID)
            return cell
        }
        let cell = (tableView.makeView(withIdentifier: id, owner: nil) as? ResultCellView) ?? makeCell(identifier: id)
        if let file = pager.row(at: row) {
            cell.configure(file, libraryName: libraryNames[file.libraryId])
        } else {
            cell.showLoading()
        }
        return cell
    }

    func tableView(_ tableView: NSTableView, rowViewForRow row: Int) -> NSTableRowView? {
        (tableView.makeView(withIdentifier: ResultsRowView.identifier, owner: nil) as? ResultsRowView) ?? {
            let view = ResultsRowView()
            view.identifier = ResultsRowView.identifier
            return view
        }()
    }

    /// A (re)used row view gets the table's hover state for its new row.
    func tableView(_ tableView: NSTableView, didAdd rowView: NSTableRowView, forRow row: Int) {
        self.tableView.applyHover(to: rowView, row: row)
    }

    func tableViewSelectionDidChange(_ notification: Notification) {
        guard !suppressSelection else { return }
        selectedIDs = tableView.selectedRowIndexes.compactMap { pager.peek($0)?.fileId }
        onSelectionChange?(selectedIDs)
        if tableView.isSelectingAll { return }
        let row = tableView.selectedRow >= 0 ? pager.peek(tableView.selectedRow) : nil
        // ⌘A / ⌘-click that keep the focus must not restart the preview.
        guard row?.fileId != selectedID else { return }
        selectedID = row?.fileId
        onSelect?(row)
    }

    // MARK: Drag out

    /// One file URL per loaded, existing row. Placeholder rows and files
    /// missing on disk (offline volume) get no writer. NSTableView drags all
    /// selected rows through this, so multi-select drags every file.
    func tableView(_ tableView: NSTableView, pasteboardWriterForRow row: Int) -> NSPasteboardWriting? {
        guard let file = pager.peek(row), FileManager.default.fileExists(atPath: file.path) else { return nil }
        return NSURL(fileURLWithPath: file.path)
    }

    func tableView(
        _ tableView: NSTableView, draggingSession session: NSDraggingSession, willBeginAt screenPoint: NSPoint,
        forRowIndexes rowIndexes: IndexSet
    ) { dragWillBegin() }

    func tableView(
        _ tableView: NSTableView, draggingSession session: NSDraggingSession, endedAt screenPoint: NSPoint,
        operation: NSDragOperation
    ) { dragEnded() }

    func dragWillBegin() { DragOutState.isDraggingFromTable = true }
    func dragEnded() { DragOutState.isDraggingFromTable = false }

    /// ⌘C: file URLs of the selection on the general pasteboard.
    private func copySelectedURLs() {
        let urls = selectedIDs.compactMap { pager.cachedRow(fileId: $0) }
            .filter { FileManager.default.fileExists(atPath: $0.path) }
            .map { NSURL(fileURLWithPath: $0.path) }
        guard !urls.isEmpty else { return }
        NSPasteboard.general.clearContents()
        NSPasteboard.general.writeObjects(urls)
    }

    // MARK: Cells

    private func makeCell(identifier id: NSUserInterfaceItemIdentifier) -> ResultCellView {
        switch id.rawValue {
        case "category": return CategoryCellView(identifier: id)
        case "kind": return KindCellView(identifier: id)
        case "bpm": return ValueCellView(identifier: id, value: .bpm)
        case "key": return ValueCellView(identifier: id, value: .key)
        case "duration": return ValueCellView(identifier: id, value: .duration)
        case "library": return ValueCellView(identifier: id, value: .library)
        default: return NameCellView(identifier: id)
        }
    }
}
