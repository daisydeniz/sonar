import Foundation

/// Tag management (T12.8, DESIGN §4.12 layer 3): rename, merge, delete, the
/// similar-pairs list and dropping rows on a sidebar tag. The core does the
/// work and returns the affected file ids; each change is ONE Undo step
/// that re-applies the previous tags to those files.
extension AppModel {
    // MARK: Data

    /// Every tag with its count and the similar-pair suggestions.
    func manageTagsSnapshot() async throws -> (tags: [TagInfo], pairs: [SimilarTagPair]) {
        guard let bridge else { return ([], []) }
        let tags = try await bridge.allTags()
        let pairs = try await bridge.similarTags()
        return (tags, pairs)
    }

    /// The stored spelling of `name` (tags are unique case-insensitively), or
    /// the typed name with collapsed spaces when it does not exist yet.
    func canonicalTag(_ name: String) -> String {
        let typed = TagManageLogic.collapsed(name)
        return organizeState.tags.first { $0.name.caseInsensitiveCompare(typed) == .orderedSame }?.name ?? typed
    }

    // MARK: Operations

    /// Renames `from` to `to`; onto an existing tag it merges into it.
    @discardableResult
    func renameTag(from: String, to: String) async -> Bool {
        let target = TagManageLogic.collapsed(to)
        guard !target.isEmpty, target != from else { return false }
        return await mergeOrRename(sources: [from], target: target, rename: true)
    }

    /// Merges the tags into `target` (created when new).
    @discardableResult
    func mergeTags(_ sources: [String], into target: String) async -> Bool {
        let sources = sources.filter { $0.caseInsensitiveCompare(target) != .orderedSame }
        guard !sources.isEmpty else { return false }
        return await mergeOrRename(sources: sources, target: target, rename: false)
    }

    /// "Merge All" of the similar-tags card: one Undo step for every group.
    @discardableResult
    func mergeSimilarPairs(_ pairs: [SimilarTagPair]) async -> Bool {
        let plan = TagManageLogic.mergePlan(pairs)
        guard !plan.isEmpty else { return false }
        var steps: [TagUndoStep] = []
        for group in plan {
            guard let step = await performMerge(sources: group.sources, target: group.target, rename: false) else {
                break
            }
            steps.append(step)
        }
        await finishTagManagement(steps, name: "Merge Similar Tags")
        return !steps.isEmpty
    }

    /// Deletes the tags from every sample after the sheet confirmed.
    @discardableResult
    func deleteTags(_ names: [String]) async -> Bool {
        guard let bridge, !names.isEmpty else { return false }
        var steps: [TagUndoStep] = []
        var failed = false
        for name in names {
            do {
                let ids = try await bridge.deleteTag(name: name)
                steps.append(TagUndoStep(
                    undo: { b in if !ids.isEmpty { _ = try await b.addTags(fileIds: ids, tags: [name]) } },
                    redo: { b in _ = try await b.deleteTag(name: name) }))
            } catch {
                errorMessage = Self.describe(error)
                failed = true
                break
            }
            if sidebarSelection == .tag(name) { sidebarSelection = .allSamples }
        }
        await finishTagManagement(steps, name: names.count == 1 ? "Delete Tag “\(names[0])”" : "Delete Tags")
        return !failed
    }

    /// "Ignore": the pair is never suggested again (the core remembers; not undoable).
    func ignoreSimilarPair(_ pair: SimilarTagPair) async {
        guard let bridge else { return }
        do { try await bridge.ignoreSimilarTags(a: pair.a, b: pair.b) } catch { errorMessage = Self.describe(error) }
    }

    /// Dropping result rows on a sidebar tag: adds it to the dragged rows
    /// (the selection, which a table drag always includes). One Undo step.
    @discardableResult
    func dropRowsOnTag(_ tag: String, ids: [Int64]? = nil) async -> Bool {
        let ids = ids ?? organizeTargetIDs
        guard !ids.isEmpty else { return false }
        return await addTagsReportingSuccess(tag, to: ids)
    }

    private func addTagsReportingSuccess(_ tag: String, to ids: [Int64]) async -> Bool {
        var state = await tagStateComplete(for: ids)
        guard let change = state.add(tag) else { return true }  // every row already has it
        return await applyTagChanges([change])
    }

    // MARK: Plumbing

    private func mergeOrRename(sources: [String], target: String, rename: Bool) async -> Bool {
        guard let step = await performMerge(sources: sources, target: target, rename: rename) else { return false }
        let name = rename ? "Rename Tag “\(sources[0])”" : "Merge Tags"
        await finishTagManagement([step], name: name)
        return true
    }

    /// Snapshots which files carry the sources (and the target), runs the
    /// core call and returns the step that undoes / redoes it; nil on failure.
    private func performMerge(sources: [String], target: String, rename: Bool) async -> TagUndoStep? {
        guard let bridge else { return nil }
        let canonical = canonicalTag(target)
        let targetExists = organizeState.tags.contains { $0.name.caseInsensitiveCompare(canonical) == .orderedSame }
        var before: [String: [Int64]] = [:]
        var targetIds: Set<Int64> = []
        do {
            for source in sources { before[source] = try await bridge.fileIds(withTag: source) }
            if targetExists { targetIds = Set(try await bridge.fileIds(withTag: canonical)) }
            let redo: @Sendable (CoreBridge) async throws -> Void = { b in
                if rename, let source = sources.first {
                    _ = try await b.renameTag(from: source, to: target)
                } else {
                    _ = try await b.mergeTags(sources: sources, target: target)
                }
            }
            try await redo(bridge)
            if let current = sidebarSelection.tagName,
                sources.contains(where: { $0.caseInsensitiveCompare(current) == .orderedSame })
            {
                sidebarSelection = .tag(canonical)
            }
            let touched = before
            let alreadyTagged = targetIds
            return TagUndoStep(
                undo: { b in
                    // Files that only had a source never had the target: take it off again.
                    let onlySources = Set(touched.values.flatMap { $0 }).subtracting(alreadyTagged)
                    if !onlySources.isEmpty { _ = try await b.removeTags(fileIds: Array(onlySources), tags: [canonical]) }
                    for (source, ids) in touched where !ids.isEmpty {
                        _ = try await b.addTags(fileIds: ids, tags: [source])
                    }
                },
                redo: redo)
        } catch {
            errorMessage = Self.describe(error)
            return nil
        }
    }

    /// Refreshes sidebar, tags and results, bumps the revision (open sheets
    /// and popovers reload) and registers the steps as one Undo.
    private func finishTagManagement(_ steps: [TagUndoStep], name: String) async {
        organizeState.tagWork += 1
        await organize { _ in }
        organizeState.tagWork -= 1
        guard !steps.isEmpty else {
            organizeState.tagsRevision += 1
            return
        }
        registerTagManageUndo(steps, name: name)
        organizeState.tagsRevision += 1
    }

    /// Undo runs the steps backwards; the handler registers the mirror so
    /// Redo works (and Undo again after it).
    private func registerTagManageUndo(_ steps: [TagUndoStep], name: String) {
        let manager = organizeState.undoManager
        manager.registerUndo(withTarget: self) { target in
            MainActor.assumeIsolated {
                target.registerTagManageUndo(steps.reversed().map(\.inverted), name: name)
                target.track { [target] in await target.runTagUndo(steps.reversed().map(\.undo)) }
            }
        }
        manager.setActionName(name)
    }

    private func runTagUndo(_ actions: [@Sendable (CoreBridge) async throws -> Void]) async {
        guard let bridge else { return }
        await organize { _ in
            for action in actions { try await action(bridge) }
        }
        organizeState.tagsRevision += 1
    }
}

/// How to take back and redo one core tag operation.
struct TagUndoStep: Sendable {
    let undo: @Sendable (CoreBridge) async throws -> Void
    let redo: @Sendable (CoreBridge) async throws -> Void

    /// The step as seen from the other side of an Undo.
    var inverted: TagUndoStep { TagUndoStep(undo: redo, redo: undo) }
}

extension SidebarItem {
    /// The tag a tag scope filters on.
    var tagName: String? {
        if case .tag(let name) = self { return name }
        return nil
    }
}
