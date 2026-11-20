import Foundation
import Observation

/// Model of the live tag popover: the pure `TagPopoverState` plus the
/// dispatch of every change to `AppModel` (core write + one Undo step).
@MainActor
@Observable
final class TagPopoverModel {
    var state: TagPopoverState
    let ids: [Int64]
    let app: AppModel

    init(app: AppModel, ids: [Int64], state: TagPopoverState? = nil) {
        self.app = app
        self.ids = ids
        self.state = state ?? app.tagState(for: ids)
        if state == nil, self.state.total < ids.count { reload() }
    }

    /// True while rows missing from the pager cache are fetched from the
    /// core; edits are held back until the state covers every file.
    private(set) var isLoading = false
    @ObservationIgnored private var loadGeneration = 0

    /// "Cymatics_Drums_Warm_17" for one file, "3 samples" for several.
    var subtitle: String {
        guard state.total == 1, let id = ids.first, let name = app.pager.cachedRow(fileId: id)?.name
        else { return "\(max(state.total, ids.count)) samples" }
        return name
    }

    var canManage: Bool { app.organizeState.manageTags != nil }

    /// Closes the popover and opens the Manage Tags sheet.
    func manage() {
        app.organizeState.tagPopover = nil
        app.organizeState.manageTags?()
    }

    /// Fresh tags after the core refresh (or an Undo from outside); the
    /// typed text stays.
    func reload() {
        loadGeneration += 1
        let generation = loadGeneration
        let cached = app.tagState(for: ids)
        if cached.total == ids.count {
            state.reload(library: app.organizeState.tags, files: cached.files.map { (id: $0.id, tags: $0.tags) })
            isLoading = false
            return
        }
        if state.total < ids.count { isLoading = true }
        Task { [self] in
            let fresh = await app.tagStateComplete(for: ids)
            guard generation == loadGeneration else { return }
            state.reload(library: app.organizeState.tags, files: fresh.files.map { (id: $0.id, tags: $0.tags) })
            isLoading = false
        }
    }

    private func apply(_ changes: [TagChange]) { app.applyTagChangesInBackground(changes) }

    func toggle(_ name: String) { apply(state.toggle(name).map { [$0] } ?? []) }
    func add(_ name: String) { apply(state.add(name).map { [$0] } ?? []) }
    func remove(_ name: String) { apply(state.remove(name).map { [$0] } ?? []) }
    func removeLast() { apply(state.removeLast().map { [$0] } ?? []) }
    func activateHighlighted() { apply(state.activateHighlighted()) }
    func commitText() { apply(state.commitText()) }
    func textChanged() { apply(state.textChanged()) }
    func moveHighlight(by delta: Int) { state.moveHighlight(by: delta) }

    /// Activates a list row by click: toggle / create, then clear the field.
    func activate(_ row: TagPopoverState.Row) {
        switch row {
        case .create(let name): add(name)
        case .tag(let name, _, _): toggle(name)
        }
        state.text = ""
    }
}
