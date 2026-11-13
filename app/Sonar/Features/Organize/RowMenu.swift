import Foundation

/// Platform-neutral description of a context-menu item, so the model decides
/// what the menu offers (and tests can inspect it) while the table controller
/// only turns entries into `NSMenuItem`s.
struct RowMenuEntry {
    var title: String
    var isSeparator = false
    var children: [RowMenuEntry] = []
    var action: (@MainActor () -> Void)?

    static var separator: RowMenuEntry { RowMenuEntry(title: "", isSeparator: true) }
}

extension AppModel {
    /// Organize items for a context menu on `rows` (the clicked rows).
    func rowMenuEntries(for rows: [FileRow]) -> [RowMenuEntry] {
        guard !rows.isEmpty else { return [] }
        let ids = rows.map(\.fileId)
        var entries: [RowMenuEntry] = []
        entries.append(RowMenuEntry(
            title: rows.allSatisfy(\.favorite) ? "Unfavorite" : "Favorite",
            action: { [weak self] in Task { await self?.toggleFavorite(rows: rows) } }))
        let unhide = rows.allSatisfy(\.hidden)
        entries.append(RowMenuEntry(
            title: unhide ? "Unhide" : "Hide",
            action: { [weak self] in Task { await self?.setHidden(fileIds: ids, on: !unhide) } }))
        entries.append(RowMenuEntry(
            title: "Add Tag…", action: { [weak self] in self?.organizeState.ask(.addTag(ids)) }))
        var collectionItems = collections.map { c in
            RowMenuEntry(
                title: c.name,
                action: { [weak self] in Task { await self?.addToCollection(id: c.id, fileIds: ids) } })
        }
        if !collectionItems.isEmpty { collectionItems.append(.separator) }
        collectionItems.append(RowMenuEntry(
            title: "New Collection…", action: { [weak self] in self?.organizeState.ask(.newCollection(ids)) }))
        entries.append(RowMenuEntry(title: "Add to Collection", children: collectionItems))
        if case .collection(let id) = sidebarSelection {
            entries.append(RowMenuEntry(
                title: "Remove from Collection",
                action: { [weak self] in Task { await self?.removeFromCollection(id: id, fileIds: ids) } }))
        }
        return entries
    }
}
