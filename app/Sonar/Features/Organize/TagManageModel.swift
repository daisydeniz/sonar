import Foundation
import Observation

/// State and actions of the Manage Tags sheet (DESIGN §4.12 layer 3). The
/// core does the work through `AppModel` (one Undo step each); this holds
/// what the sheet shows: tags, similar pairs, filter, selection, inline
/// rename and the pending delete.
@MainActor
@Observable
final class TagManageModel {
    enum Phase: Equatable {
        case loading, ready
        case failed(String)
    }

    let app: AppModel
    private(set) var tags: [TagInfo] = []
    private(set) var pairs: [SimilarTagPair] = []
    private(set) var phase: Phase = .loading
    /// A change is running; the sheet disables its actions meanwhile.
    private(set) var isBusy = false

    var filter = ""
    var selection: Set<String> = []
    /// The tag being renamed inline, and the text field's content.
    var renaming: String?
    var renameText = ""
    /// A new tag being typed (the `+` row) and its text.
    var isCreating = false
    var createText = ""
    var confirmingDelete = false
    @ObservationIgnored private var loadGeneration = 0
    /// Injected data (snapshots): `reload` leaves it alone.
    @ObservationIgnored private let isStatic: Bool

    /// `tags` / `pairs` inject data (snapshots, tests); nil loads from the core.
    init(app: AppModel, tags: [TagInfo]? = nil, pairs: [SimilarTagPair]? = nil) {
        self.app = app
        isStatic = tags != nil
        if let tags {
            self.tags = TagManageLogic.sorted(tags)
            self.pairs = pairs ?? []
            phase = .ready
        }
    }

    // MARK: Derived

    var visibleTags: [TagInfo] { TagManageLogic.filtered(tags, by: filter) }

    /// "10 tags · All libraries".
    var subtitle: String { "\(tags.count) \(tags.count == 1 ? "tag" : "tags") · All libraries" }

    /// "1 selected · double-click to rename".
    var hint: String {
        selection.isEmpty ? "Double-click a tag to rename it" : "\(selection.count) selected · double-click to rename"
    }

    /// Selected tags in list order.
    var selectedTags: [TagInfo] { tags.filter { selection.contains($0.name) } }

    var canMerge: Bool { selection.count >= 2 && !isBusy }
    var canDelete: Bool { !selection.isEmpty && !isBusy }
    /// `+` adds a tag to the samples selected in the results.
    var canCreate: Bool { !app.organizeTargetIDs.isEmpty && !isBusy }

    /// "“warm” will be removed from 58 samples." for the delete confirmation.
    var deleteMessage: String {
        let chosen = selectedTags
        let total = chosen.reduce(0) { $0 + Int($1.files) }
        if chosen.count == 1, let one = chosen.first {
            return total == 0
                ? "“\(one.name)” is not used by any sample."
                : "“\(one.name)” will be removed from \(total) \(total == 1 ? "sample" : "samples"). The samples stay."
        }
        return "\(chosen.count) tags will be removed from the samples that have them (\(total) in all). The samples stay."
    }

    var deleteTitle: String {
        selection.count == 1 ? "Delete this tag?" : "Delete \(selection.count) tags?"
    }

    // MARK: Loading

    func reload() async {
        guard !isStatic else { return }
        loadGeneration += 1
        let mine = loadGeneration
        if tags.isEmpty { phase = .loading }
        do {
            let snapshot = try await app.manageTagsSnapshot()
            guard mine == loadGeneration else { return }
            tags = TagManageLogic.sorted(snapshot.tags)
            pairs = snapshot.pairs
            selection.formIntersection(Set(tags.map(\.name)))
            phase = .ready
        } catch {
            guard mine == loadGeneration else { return }
            phase = .failed(AppModel.describe(error))
        }
    }

    // MARK: Actions

    private func perform(_ work: () async -> Void) async {
        isBusy = true
        await work()
        isBusy = false
        await reload()
    }

    func beginRename(_ name: String) {
        renaming = name
        renameText = name
        selection = [name]
    }

    func cancelRename() { renaming = nil }

    /// Return in the inline field: renames (onto an existing name = merge).
    func commitRename() async {
        guard let from = renaming else { return }
        let to = renameText
        renaming = nil
        guard TagManageLogic.collapsed(to) != from else { return }
        await perform {
            if await app.renameTag(from: from, to: to) { selection = [app.canonicalTag(to)] }
        }
    }

    func beginCreate() {
        guard canCreate else { return }
        createText = ""
        isCreating = true
    }

    func commitCreate() async {
        let name = TagManageLogic.collapsed(createText)
        isCreating = false
        guard !name.isEmpty else { return }
        await perform { await app.addTags(names: [name]) }
        selection = [app.canonicalTag(name)]
    }

    /// Merge… with the chosen surviving name.
    func mergeSelection(into target: String) async {
        let sources = selectedTags.map(\.name)
        await perform {
            if await app.mergeTags(sources, into: target) { selection = [app.canonicalTag(target)] }
        }
    }

    func deleteSelection() async {
        let names = selectedTags.map(\.name)
        await perform {
            if await app.deleteTags(names) { selection = [] }
        }
    }

    func merge(_ pair: SimilarTagPair) async {
        let (keep, drop) = TagManageLogic.survivor(of: pair)
        await perform { await app.mergeTags([drop], into: keep) }
    }

    func mergeAll() async {
        let all = pairs
        await perform { await app.mergeSimilarPairs(all) }
    }

    func ignore(_ pair: SimilarTagPair) async {
        await perform { await app.ignoreSimilarPair(pair) }
    }
}
