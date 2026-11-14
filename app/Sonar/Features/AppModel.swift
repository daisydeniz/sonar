import Foundation
import Observation

/// What the sidebar has selected; decides which rows the results area shows.
enum SidebarItem: Hashable, Sendable {
    case allSamples
    case favorites
    case library(Int64)
    case collection(Int64)
    /// Duplicate finder (T6.9b): its own view, not a result list.
    case duplicates
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
    var engineState: EngineState = .opening  // set by `retryOpen` (extension)
    private(set) var libraries: [LibraryInfo] = []
    private(set) var collections: [CollectionInfo] = []
    /// Lazily paged results behind the table (latest-wins via its generations).
    let pager = ResultsPager()
    /// Audio preview (auto-play, cache, pitch, loop).
    let preview: AudioPreview
    /// Waveform of the selected row (peaks from the core, zoom viewport).
    let waveform = WaveformModel()
    /// Similarity mode (seed, sliders, status); see `Features/Similar`.
    let similarity = SimilarityModel()
    /// Organize state (hidden switch, prompts, override error); see `Features/Organize`.
    let organizeState = OrganizeState()
    /// Duplicates view state; see `Features/Duplicates`.
    let duplicates = DuplicatesModel()
    /// Persisted app settings (indexing options); see `Features/Settings`.
    let settings: AppSettings
    /// Non-modal, dismissible engine/watcher error message (coalesced).
    let banner = StatusBanner()
    /// Refreshes in flight / first sidebar and results loads done (T7.2 empty states).
    var loadingRefreshes = 0
    private(set) var sidebarLoaded = false
    private(set) var resultsLoadedOnce = false
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

    @ObservationIgnored private(set) var bridge: CoreBridge?
    @ObservationIgnored var generation = 0
    @ObservationIgnored private var lastAppliedSort: SortKey = .relevance
    @ObservationIgnored private var lastAppliedDescending = false
    @ObservationIgnored private var pollTask: Task<Void, Never>?
    @ObservationIgnored private var wasRunning = false
    @ObservationIgnored var isStarted = false
    @ObservationIgnored private let activePollNs: UInt64
    @ObservationIgnored private let idlePollNs: UInt64
    @ObservationIgnored private let opener: @Sendable () async throws -> CoreBridge

    /// Facet counts of the current query (nil for collections / semantic mode).
    var facets: SearchFacets? { pager.facets }
    /// Library chips are redundant while the sidebar picks one library.
    var showsLibraryChips: Bool {
        if case .allSamples = sidebarSelection { return true }
        return sidebarSelection == .favorites
    }
    /// Filters do not apply to a collection's own contents.
    var showsFilterBar: Bool {
        switch sidebarSelection {
        case .collection, .duplicates: return false
        default: return true
        }
    }

    /// Clears every filter chip and re-queries.
    func clearFilters() { filters = FilterState() }

    /// Space in the table (wired to `AudioPreview.toggle`).
    func togglePreview() { keyboard.togglePreview() }
    /// Return in the table: reveal the selected file in Finder.
    func revealSelected() { keyboard.revealSelected(selectedRow, selection: selectedRows) }

    /// Ids of every selected (loaded) row in table order; the focused file
    /// (`selectedRow`) is the most recently clicked one. T6.8 acts on this.
    var selectedFileIDs: [Int64] = []
    /// Cached rows of `selectedFileIDs`.
    var selectedRows: [FileRow] { selectedFileIDs.compactMap { pager.cachedRow(fileId: $0) } }

    var selectedFileId: Int64? { selectedRow?.fileId }

    /// Replaces the selected row's snapshot with a fresher copy (same file).
    func updateSelectedSnapshot(_ row: FileRow) { selectedRow = row }

    /// Rows in the current result list.
    var resultCount: Int { pager.count }

    /// Table selection changed (nil = nothing selected): updates the preview
    /// (auto-play + pre-decode of the cached neighbours, nearest first).
    /// `neighbours` overrides the pager lookup (Duplicates view: the group).
    func select(_ row: FileRow?, neighbours explicit: [FileRow]? = nil) {
        selectedRow = row
        preview.select(row, neighbours: explicit ?? neighbours(of: row))
        waveform.load(fileId: row?.fileId)
    }

    /// Cached rows within `AudioPreview.neighbourReach` of `row` (no fetches).
    private func neighbours(of row: FileRow?) -> [FileRow] {
        guard let row, let i = pager.index(ofFileId: row.fileId) else { return [] }
        return (1...AudioPreview.neighbourReach).flatMap { d in [pager.peek(i + d), pager.peek(i - d)] }
            .compactMap { $0 }
    }

    /// A header click: the core re-sorts, the table restarts at the top.
    func setSort(_ key: SortKey, descending: Bool) async {
        // Similarity results stay in distance order.
        guard !similarity.isActive, key != sortKey || descending != sortDescending else { return }
        sortKey = key
        sortDescending = descending
        await refreshResults(keepScroll: false)
    }

    /// `engine` is injectable (tests pass one opened on a temp home); `nil`
    /// opens the default home in `start()`.
    init(
        engine: SonarEngine? = nil, preview: AudioPreview = AudioPreview(), activePollMs: UInt64 = 250,
        idlePollMs: UInt64 = 1000, settings: AppSettings = AppSettings(),
        opener: @escaping @Sendable () async throws -> CoreBridge = { try await CoreBridge.openDefault() }
    ) {
        self.settings = settings
        self.opener = opener
        self.preview = preview
        bridge = engine.map(CoreBridge.init(engine:))
        if let engine {
            preview.decoder = CoreDecoder(engine: engine)
            waveform.provider = CoreWaveformProvider(engine: engine)
        }
        keyboard.onTogglePreview = { [weak preview] in preview?.toggle() }
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
            if bridge == nil {
                bridge = try await opener()
                if let engine = bridge?.engine {
                    preview.decoder = CoreDecoder(engine: engine)
                    waveform.provider = CoreWaveformProvider(engine: engine)
                }
            }
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
        preview.warmUp()
        if !libraries.isEmpty {
            do {
                try bridge?.startIndexing(includeEmbeddings: settings.includeEmbeddings)
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
            sidebarLoaded = true
            collections = try await bridge.collections()
            organizeState.tags = try await bridge.allTags()  // suggestions, loaded with the sidebar
        } catch {
            errorMessage = Self.describe(error)
        }
    }

    /// Adds a folder as a library: scan, watch, start indexing.
    func addLibrary(path: String) async {
        guard let bridge else { return }
        do {
            _ = try await bridge.addLibrary(path: path, includeEmbeddings: settings.includeEmbeddings)
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
    func track(_ body: @escaping @MainActor () async -> Void) {
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
        if similarity.isActive { return await refreshSimilar(keepScroll: keepScroll) }
        guard let bridge, sidebarSelection != .duplicates else { return }
        loadingRefreshes += 1
        defer {
            loadingRefreshes -= 1
            resultsLoadedOnce = true
        }
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
            case .duplicates:
                return
            case .allSamples, .favorites, .library:
                var filters = SearchFilters()
                if selection == .favorites { filters.favoritesOnly = true }
                filters.includeHidden = organizeState.showHidden
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
                select(nil)
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
        for event in bridge.pollEvents() where apply(event) { refresh = true }
        wasRunning = progress.running
        if refresh {
            track {
                await self.reloadSidebar()
                await self.refreshResults()
            }
        }
    }

    static func describe(_ error: Error) -> String {
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
