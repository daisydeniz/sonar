import Foundation

/// Tags (T12.7, DESIGN §4.12): live edits with one Undo step each, the
/// "Tags ›" menu entries and the popover request. Normalisation and storage
/// are the core's; the state that decides what changes is `TagPopoverState`.
extension AppModel {
    // MARK: State

    /// Cached row of a file (the focused snapshot when the pager lacks it).
    private func tagRow(_ id: Int64) -> FileRow? {
        pager.cachedRow(fileId: id) ?? (selectedRow?.fileId == id ? selectedRow : nil)
    }

    /// State from the cached rows only; rows the pager evicted are missing
    /// (`total < ids.count`). Menus use it; edits use `tagStateComplete`.
    func tagState(for ids: [Int64]) -> TagPopoverState {
        TagPopoverState(
            library: organizeState.tags,
            files: ids.compactMap { id in tagRow(id).map { (id: id, tags: $0.tags) } })
    }

    /// State for ALL ids: rows missing from the pager cache are fetched from
    /// the core off the main thread.
    func tagStateComplete(for ids: [Int64]) async -> TagPopoverState {
        let cached = tagState(for: ids)
        guard cached.total < ids.count, let bridge else { return cached }
        let have = Set(cached.files.map(\.id))
        let missing = ids.filter { !have.contains($0) }
        let fetched = (try? await bridge.rows(fileIds: missing)) ?? []
        let byId = Dictionary(fetched.map { ($0.fileId, $0.tags) }, uniquingKeysWith: { a, _ in a })
        let files = ids.compactMap { id in
            tagRow(id).map { (id: id, tags: $0.tags) } ?? byId[id].map { (id: id, tags: $0) }
        }
        return TagPopoverState(library: organizeState.tags, files: files)
    }

    // MARK: Applying

    /// Applies live tag edits to the core; on success registers ONE Undo step
    /// for all of them. Undo / Redo re-apply the inverse without growing the
    /// stack beyond the system's undo/redo handling.
    @discardableResult
    func applyTagChanges(_ changes: [TagChange], registerUndo: Bool = true) async -> Bool {
        let changes = changes.filter { !$0.fileIds.isEmpty }
        guard !changes.isEmpty else { return true }
        organizeState.tagWork += 1
        let ok = await organize { bridge in
            for change in changes {
                switch change.kind {
                case .add: try await bridge.addTags(fileIds: change.fileIds, tags: [change.tag])
                case .remove: try await bridge.removeTags(fileIds: change.fileIds, tags: [change.tag])
                }
            }
        }
        organizeState.tagWork -= 1
        if ok, registerUndo { registerTagUndo(changes) }
        if organizeState.tagWork == 0 { organizeState.tagsRevision += 1 }
        return ok
    }

    /// Fire-and-forget form for views and menus (tracked so tests can `settle`).
    func applyTagChangesInBackground(_ changes: [TagChange]) {
        guard !changes.isEmpty else { return }
        track { [self] in await applyTagChanges(changes) }
    }

    /// Registers the Undo of `applied`. While the manager is undoing, the
    /// same call inside the handler lands on the redo stack.
    private func registerTagUndo(_ applied: [TagChange]) {
        let manager = organizeState.undoManager
        manager.registerUndo(withTarget: self) { target in
            let inverse = TagChange.inverse(of: applied)
            MainActor.assumeIsolated {
                target.registerTagUndo(inverse)
                target.track { [target] in await target.applyTagChanges(inverse, registerUndo: false) }
            }
        }
        manager.setActionName(Self.undoName(applied))
    }

    private static func undoName(_ changes: [TagChange]) -> String {
        guard changes.count == 1, let change = changes.first else { return "Edit Tags" }
        return change.kind == .add ? "Add Tag “\(change.tag)”" : "Remove Tag “\(change.tag)”"
    }

    // MARK: Convenience (inspector chips, tests)

    /// Adds comma-separated tags to the files (default: the selection).
    func addTags(_ text: String, to ids: [Int64]? = nil) async {
        await addTags(names: Self.parseTags(text), to: ids)
    }

    /// Adds already-split tags (a name is never re-split).
    func addTags(names tags: [String], to ids: [Int64]? = nil) async {
        let ids = ids ?? organizeTargetIDs
        var state = await tagStateComplete(for: ids)
        await applyTagChanges(tags.compactMap { state.add($0) })
    }

    /// Removes one tag from every file of the selection.
    func removeTag(_ tag: String) async {
        var state = await tagStateComplete(for: organizeTargetIDs)
        if let change = state.remove(tag) { await applyTagChanges([change]) }
    }

    // MARK: Menu

    /// "Tags ›" for a context menu or the Edit menu: the 8 most used tags
    /// with a tri-state mark for `ids` (✓ all, – some), a separator, Add Tag…
    /// (⌘T) and Manage Tags… (opens the sheet).
    func tagMenuEntry(for ids: [Int64]) -> RowMenuEntry {
        let state = tagState(for: ids)
        let complete = state.total == ids.count
        var children: [RowMenuEntry] = []
        for case .tag(let name, _, let mark) in state.mostUsed {
            var item = RowMenuEntry(title: name, mark: mark.menuMark)
            if complete {
                item.action = { [weak self] in
                    var state = state
                    if let change = state.toggle(name) { self?.applyTagChangesInBackground([change]) }
                }
            }
            children.append(item)
        }
        if !children.isEmpty { children.append(.separator) }
        children.append(RowMenuEntry(
            title: "Add Tag…", shortcut: "t",
            action: { [weak self] in self?.showTagPopover(ids: ids) }))
        children.append(RowMenuEntry(
            title: "Manage Tags…", action: { [weak self] in self?.organizeState.manageTags?() }))
        return RowMenuEntry(title: "Tags", children: children)
    }

    // MARK: Popover

    /// Opens the tag popover for these files (default: the selection),
    /// anchored to the focused row unless told otherwise.
    func showTagPopover(ids: [Int64]? = nil, anchor: TagPopoverAnchor = .row) {
        let ids = ids ?? organizeTargetIDs
        guard !ids.isEmpty else { return }
        organizeState.tagPopover = TagPopoverRequest(ids: ids, anchor: anchor)
    }
}

extension TagMark {
    /// Menu mark: ✓ all, – some.
    var menuMark: MenuMark {
        switch self {
        case .none: return .off
        case .some: return .mixed
        case .all: return .on
        }
    }
}
