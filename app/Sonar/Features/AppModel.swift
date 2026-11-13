import Foundation
import Observation

/// What the sidebar has selected; decides which rows the results area shows.
enum SidebarItem: Hashable, Sendable {
    case allSamples
    case favorites
    case library(Int64)
    case collection(Int64)
}

/// Whether the engine is available.
enum EngineState: Equatable, Sendable {
    case opening
    case ready
    case failed(String)
}

/// Main-actor state of the app shell. A thin client of the Rust core: it
/// holds what the views render and forwards user actions to `CoreBridge`
/// (every blocking call runs off the main actor).
///
/// Search is latest-wins: each `refreshResults()` takes a new generation; a
/// result is applied only if it is still the newest and the core did not
/// throw `FfiError.Cancelled` (ADR-032).
@MainActor
@Observable
final class AppModel {
    // MARK: Rendered state
    private(set) var engineState: EngineState = .opening
    private(set) var libraries: [LibraryInfo] = []
    private(set) var collections: [CollectionInfo] = []
    /// Lazily paged results behind the table (latest-wins via its generations).
    let pager = ResultsPager()
    private(set) var progress = IndexingProgress(
        enabled: false, running: false, paused: false, stage: nil, total: 0, done: 0, errors: 0)
    /// Last user-visible error (add/remove/search failures); nil when none.
    var errorMessage: String?

    var sidebarSelection: SidebarItem = .allSamples
    var searchText = ""
    /// Filter chips (core-filtered; changing them re-queries via `scheduleRefresh`).
    var filters = FilterState()
    /// Keyboard hooks (preview toggle, reveal, focus moves).
    let keyboard = KeyboardActions()
    /// Selected row (a snapshot, so it survives page eviction and resets).
    private(set) var selectedRow: FileRow?
    /// Core sort key of the table; `.relevance` = the core's default order.
    private(set) var sortKey: SortKey = .relevance
    private(set) var sortDescending = false

    @ObservationIgnored private var bridge: CoreBridge?
    @ObservationIgnored private var generation = 0
    @ObservationIgnored private var lastAppliedSort: SortKey = .relevance
    @ObservationIgnored private var lastAppliedDescending = false
    @ObservationIgnored private var pollTask: Task<Void, Never>?
    @ObservationIgnored private var wasRunning = false
    @ObservationIgnored private var isStarted = false
    @ObservationIgnored private let activePollNs: UInt64
    @ObservationIgnored private let idlePollNs: UInt64

    /// Facet counts of the current query (nil for collections / semantic mode).
    var facets: SearchFacets? { pager.facets }
    /// Library chips are redundant while the sidebar picks one library.
    var showsLibraryChips: Bool {
        if case .allSamples = sidebarSelection { return true }
        return sidebarSelection == .favorites
    }
    /// Filters do not apply to a collection's own contents.
    var showsFilterBar: Bool {
        if case .collection = sidebarSelection { return false }
        return true
    }

    /// Clears every filter chip and re-queries.
    func clearFilters() { filters = FilterState() }

    /// Space in the table; T6.4 wires the preview to `keyboard.onTogglePreview`.
    func togglePreview() { keyboard.togglePreview() }
    /// Return in the table: reveal the selected file in Finder.
    func revealSelected() { keyboard.revealSelected(selectedRow) }

    var selectedFileId: Int64? { selectedRow?.fileId }

    /// Rows in the current result list.
    var resultCount: Int { pager.count }

    /// Table selection changed (nil = nothing selected).
    func select(_ row: FileRow?) { selectedRow = row }

    /// A header click: the core re-sorts, the table restarts at the top.
    func setSort(_ key: SortKey, descending: Bool) async {
        guard key != sortKey || descending != sortDescending else { return }
        sortKey = key
        sortDescending = descending
        await refreshResults(keepScroll: false)
    }

    /// `engine` is injectable (tests pass one opened on a temp home); `nil`
    /// opens the default home in `start()`.
    init(engine: SonarEngine? = nil, activePollMs: UInt64 = 250, idlePollMs: UInt64 = 1000) {
        bridge = engine.map(CoreBridge.init(engine:))
        activePollNs = activePollMs * 1_000_000
        idlePollNs = idlePollMs * 1_000_000
        pager.onError = { [weak self] in self?.errorMessage = Self.describe($0) }
    }

    // MARK: Lifecycle

    /// Opens the engine if needed, loads sidebar data and results, starts the
    /// polling loop. Failure to open becomes `engineState = .failed`.
    /// Safe to call more than once (only the first call does anything). On
    /// relaunch, an interrupted indexing job is resumed when libraries exist.
    func start() async {
        guard !isStarted else { return }
        isStarted = true
        do {
            if bridge == nil { bridge = try await CoreBridge.openDefault() }
            engineState = .ready
        } catch {
            engineState = .failed(Self.describe(error))
            return
        }
        do {
            try await bridge?.startWatching()
        } catch {
            errorMessage = Self.describe(error)  // engine works; only watching failed
        }
        await reloadSidebar()
        if !libraries.isEmpty {
            do {
                try bridge?.startIndexing(includeEmbeddings: true)
            } catch {
                errorMessage = Self.describe(error)
            }
        }
        await refreshResults()
        startPolling()
    }

    /// Stops the polling loop.
    func stop() {
        pollTask?.cancel()
        pollTask = nil
    }

    // MARK: Sidebar data

    /// Reloads libraries and collections.
    func reloadSidebar() async {
        guard let bridge else { return }
        do {
            libraries = try await bridge.libraries()
            collections = try await bridge.collections()
        } catch {
            errorMessage = Self.describe(error)
        }
    }

    /// Adds a folder as a library: scan, watch, start indexing.
    func addLibrary(path: String) async {
        guard let bridge else { return }
        do {
            _ = try await bridge.addLibrary(path: path, includeEmbeddings: true)
        } catch {
            errorMessage = Self.describe(error)
        }
        await reloadSidebar()
        await refreshResults()
        pollOnce()
    }

    /// Removes a library from Sonar (never from disk).
    func removeLibrary(id: Int64) async {
        guard let bridge else { return }
        do {
            _ = try await bridge.removeLibrary(id: id)
        } catch {
            errorMessage = Self.describe(error)
        }
        if sidebarSelection == .library(id) { sidebarSelection = .allSamples }
        await reloadSidebar()
        await refreshResults()
    }

    /// Favourite on/off for files, then refresh.
    func setFavorite(fileIds: [Int64], on: Bool) async {
        guard let bridge else { return }
        do {
            try await bridge.setFavorite(fileIds: fileIds, on: on)
            if var row = selectedRow, fileIds.contains(row.fileId) {
                row.favorite = on
                selectedRow = row
            }
        } catch {
            errorMessage = Self.describe(error)
        }
        await refreshResults()
    }

    // MARK: Results

    /// Fire-and-forget refresh for view callbacks (typing, selection).
    func scheduleRefresh() {
        track { await self.refreshResults(keepScroll: false) }
    }

    /// Fire-and-forget refreshes started by `scheduleRefresh` / `pollOnce`,
    /// kept so `settle()` can await them (tests; latest-wins is unchanged).
    @ObservationIgnored private var scheduled: [UUID: Task<Void, Never>] = [:]

    /// Runs `body` as a tracked Task that removes itself when done.
    private func track(_ body: @escaping @MainActor () async -> Void) {
        let id = UUID()
        // The Task starts after this synchronous code, so the insert comes first.
        scheduled[id] = Task {
            await body()
            self.scheduled[id] = nil
        }
    }

    /// Awaits every refresh scheduled so far (including ones they schedule).
    func settle() async {
        while !scheduled.isEmpty {
            for t in Array(scheduled.values) { await t.value }
        }
    }

    /// Runs the current sidebar selection + search text off the main actor
    /// and applies the result unless a newer request superseded it.
    func refreshResults(keepScroll: Bool = true) async {
        guard let bridge else { return }
        generation += 1
        let mine = generation
        let selection = sidebarSelection
        let text = searchText
        let chips = filters
        let key = sortKey
        let descending = sortDescending
        let sortChanged = key != lastAppliedSort || descending != lastAppliedDescending
        do {
            let loader: ResultPageLoader
            switch selection {
            case .collection(let id):
                let rows = try await bridge.collectionRows(id: id)
                guard mine == generation else { return }  // a newer request exists
                loader = ArrayResultLoader(rows: rows)
            case .allSamples, .favorites, .library:
                var filters = SearchFilters()
                if selection == .favorites { filters.favoritesOnly = true }
                if case .library(let id) = selection { filters.libraryIds = [id] }
                loader = EngineResultLoader(
                    bridge: bridge,
                    query: SearchQuery(
                        text: text, filters: chips.apply(to: filters),
                        sort: key == .relevance ? nil : key, descending: descending),
                    wantsFacets: true)
            }
            let applied = try await pager.reset(
                loader: loader, preservePosition: keepScroll && !sortChanged)
            guard applied, mine == generation else { return }
            lastAppliedSort = key
            lastAppliedDescending = descending
            if pager.count == 0 {
                selectedRow = nil
            } else if let id = selectedFileId, let fresh = pager.cachedRow(fileId: id) {
                selectedRow = fresh
            }
        } catch FfiError.Cancelled {
            return  // superseded in the core; drop
        } catch {
            guard mine == generation else { return }
            errorMessage = Self.describe(error)
        }
    }

    // MARK: Indexing

    /// Pauses the indexing job (finished files stay committed).
    func pauseIndexing() { bridge?.pauseIndexing(); pollOnce() }
    /// Resumes a paused indexing job.
    func resumeIndexing() { bridge?.resumeIndexing(); pollOnce() }

    private func startPolling() {
        pollTask?.cancel()
        pollTask = Task { [weak self] in
            while !Task.isCancelled {
                guard let self else { return }
                self.pollOnce()
                let ns = self.progress.running ? self.activePollNs : self.idlePollNs
                try? await Task.sleep(nanoseconds: ns)
            }
        }
    }

    /// Reads progress (cheap), drains events, and refreshes libraries and
    /// results when something relevant happened (index refreshed, job
    /// finished, library scanned/offline/online) or a job just ended.
    func pollOnce() {
        guard let bridge else { return }
        progress = bridge.indexingProgress()
        var refresh = wasRunning && !progress.running
        for event in bridge.pollEvents() {
            switch event {
            case .libraryScanned, .libraryOffline, .libraryOnline, .indexRefreshed, .indexingFinished:
                refresh = true
            case .error(let message):
                errorMessage = message
            case .indexingStarted:
                break
            }
        }
        wasRunning = progress.running
        if refresh {
            track {
                await self.reloadSidebar()
                await self.refreshResults()
            }
        }
    }

    private static func describe(_ error: Error) -> String {
        if let e = error as? FfiError {
            switch e {
            case .NotFound(let m), .InvalidInput(let m), .Io(let m), .Audio(let m), .Internal(let m):
                return m
            case .Cancelled:
                return "Cancelled"
            }
        }
        return error.localizedDescription
    }
}

extension FileRow: Identifiable {
    public var id: Int64 { fileId }
}
