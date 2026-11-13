import Foundation

/// Organize actions (favorites, hide, tags, collections, overrides). Every
/// action takes explicit file ids (context menu: the clicked rows) or acts on
/// `organizeTargetIDs` (inspector, shortcuts): the whole multi-selection, or
/// the focused row. Product rules (validation, never overwriting overrides on
/// re-analysis) live in the core; this only forwards and refreshes.
extension AppModel {
    // MARK: Targets

    /// The multi-selection, falling back to the focused row.
    var organizeTargetIDs: [Int64] {
        if !selectedFileIDs.isEmpty { return selectedFileIDs }
        return selectedFileId.map { [$0] } ?? []
    }

    /// Cached rows of `organizeTargetIDs` (the focused snapshot if not cached).
    var organizeTargetRows: [FileRow] {
        let rows = selectedRows
        if !rows.isEmpty { return rows }
        return selectedRow.map { [$0] } ?? []
    }

    /// Splits "a, b,,c" into trimmed, de-duplicated, non-empty tags.
    static func parseTags(_ text: String) -> [String] {
        var seen = Set<String>()
        return text.split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces) }
            .filter { !$0.isEmpty && seen.insert($0).inserted }
    }

    /// Runs `work` off the main actor, reports a failure through
    /// `errorMessage`, then refreshes sidebar counts, tag suggestions,
    /// results and the focused row (scroll position and selection stay).
    @discardableResult
    func organize(_ work: (CoreBridge) async throws -> Void) async -> Bool {
        guard let bridge else { return false }
        var ok = true
        do {
            try await work(bridge)
        } catch {
            ok = false
            errorMessage = Self.describe(error)
        }
        await reloadSidebar()
        await refreshResults()
        return ok
    }

    // MARK: Favorite

    /// Favorite on, or off when every one of `rows` already is.
    func toggleFavorite(rows: [FileRow]) async {
        guard !rows.isEmpty else { return }
        await setFavorite(fileIds: rows.map(\.fileId), on: !rows.allSatisfy(\.favorite))
    }

    /// Shortcut / inspector heart: toggles the selection.
    func toggleFavoriteOfSelection() async { await toggleFavorite(rows: organizeTargetRows) }

    // MARK: Hide

    /// Hides or unhides files. Hidden files leave the results (unless
    /// "Show Hidden Samples" is on), so the selection is cleared then.
    func setHidden(fileIds: [Int64], on: Bool) async {
        guard !fileIds.isEmpty else { return }
        await organize { try await $0.setHidden(fileIds: fileIds, on: on) }
        guard on, !organizeState.showHidden else { return }
        // Drop what the reloaded results no longer contain (the hidden files).
        selectedFileIDs.removeAll { fileIds.contains($0) && pager.index(ofFileId: $0) == nil }
        if let id = selectedFileId, fileIds.contains(id), pager.index(ofFileId: id) == nil {
            select(nil)
        }
    }

    /// Shortcut: hide the selection, or unhide it when all of it is hidden.
    func toggleHiddenOfSelection() async {
        let rows = organizeTargetRows
        guard !rows.isEmpty else { return }
        await setHidden(fileIds: rows.map(\.fileId), on: !rows.allSatisfy(\.hidden))
    }

    // MARK: Tags

    /// Adds comma-separated tags to the files (default: the selection).
    func addTags(_ text: String, to ids: [Int64]? = nil) async {
        await addTags(names: Self.parseTags(text), to: ids)
    }

    /// Adds already-split tags (suggestions: a name is never re-split).
    func addTags(names tags: [String], to ids: [Int64]? = nil) async {
        let ids = ids ?? organizeTargetIDs
        guard !tags.isEmpty, !ids.isEmpty else { return }
        await organize { try await $0.addTags(fileIds: ids, tags: tags) }
    }

    /// Removes one tag from every file of the selection.
    func removeTag(_ tag: String) async {
        let ids = organizeTargetIDs
        guard !ids.isEmpty else { return }
        await organize { try await $0.removeTags(fileIds: ids, tags: [tag]) }
    }

    // MARK: Collections

    /// Creates a collection and adds `fileIds` to it. Returns its id.
    @discardableResult
    func createCollection(name: String, adding fileIds: [Int64] = []) async -> Int64? {
        let name = name.trimmingCharacters(in: .whitespaces)
        guard !name.isEmpty else { return nil }
        var created: Int64?
        await organize { bridge in
            let id = try await bridge.createCollection(name: name)
            created = id
            if !fileIds.isEmpty { try await bridge.addToCollection(id: id, fileIds: fileIds) }
        }
        return created
    }

    func renameCollection(id: Int64, name: String) async {
        let name = name.trimmingCharacters(in: .whitespaces)
        guard !name.isEmpty else { return }
        await organize { try await $0.renameCollection(id: id, name: name) }
    }

    /// Deletes the collection (its files stay in the library).
    func deleteCollection(id: Int64) async {
        if sidebarSelection == .collection(id) { sidebarSelection = .allSamples }
        await organize { try await $0.deleteCollection(id: id) }
    }

    func addToCollection(id: Int64, fileIds: [Int64]) async {
        guard !fileIds.isEmpty else { return }
        await organize { try await $0.addToCollection(id: id, fileIds: fileIds) }
    }

    func removeFromCollection(id: Int64, fileIds: [Int64]) async {
        guard !fileIds.isEmpty else { return }
        await organize { try await $0.removeFromCollection(id: id, fileIds: fileIds) }
    }

    // MARK: Prompts

    /// OK in the text alert: runs the pending prompt with the typed text.
    func commitPrompt() async {
        guard let prompt = organizeState.prompt else { return }
        let text = organizeState.promptText
        organizeState.prompt = nil
        switch prompt {
        case .addTag(let ids): await addTags(text, to: ids)
        case .newCollection(let ids): await createCollection(name: text, adding: ids)
        case .renameCollection(let id): await renameCollection(id: id, name: text)
        }
    }

    // MARK: Overrides

    /// Overrides BPM / key / category / kind of the selection. A value the
    /// core rejects sets `organizeState.overrideError` (inline, not modal)
    /// and returns false so the field can revert.
    @discardableResult
    func setOverride(_ field: OverrideField, value: String) async -> Bool {
        let ids = organizeTargetIDs
        guard let bridge, !ids.isEmpty else { return false }
        organizeState.overrideError = nil
        do {
            try await bridge.setOverride(fileIds: ids, field: field, value: value)
        } catch {
            organizeState.overrideError = Self.describe(error)
            return false
        }
        await organize { _ in }
        return true
    }

    /// "Revert to detected": clears the override (no-op when there is none).
    func revertOverride(_ field: OverrideField) async {
        let ids = organizeTargetIDs
        guard !ids.isEmpty else { return }
        organizeState.overrideError = nil
        await organize { try await $0.clearOverride(fileIds: ids, field: field) }
    }
}
