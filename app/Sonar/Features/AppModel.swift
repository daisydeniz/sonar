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
    private(set) var rows: [FileRow] = []
    /// Text matches of the last search (row count for collection views).
    private(set) var total: UInt64 = 0
    private(set) var progress = IndexingProgress(
        enabled: false, running: false, paused: false, stage: nil, total: 0, done: 0, errors: 0)
    /// Last user-visible error (add/remove/search failures); nil when none.
    var errorMessage: String?

    var sidebarSelection: SidebarItem = .allSamples
    var searchText = ""
    var selectedFileId: Int64?

    /// Page size of the simple results table (T6.2 replaces it with paging).
    static let pageLimit: UInt32 = 500

    @ObservationIgnored private var bridge: CoreBridge?
    @ObservationIgnored private var generation = 0
    @ObservationIgnored private var pollTask: Task<Void, Never>?
    @ObservationIgnored private var wasRunning = false
    @ObservationIgnored private var isStarted = false
    @ObservationIgnored private let activePollNs: UInt64
    @ObservationIgnored private let idlePollNs: UInt64

    /// The row selected in the table, if it is in the current page.
    var selectedRow: FileRow? {
        guard let id = selectedFileId else { return nil }
        return rows.first { $0.fileId == id }
    }

    /// `engine` is injectable (tests pass one opened on a temp home); `nil`
    /// opens the default home in `start()`.
    init(engine: SonarEngine? = nil, activePollMs: UInt64 = 250, idlePollMs: UInt64 = 1000) {
        bridge = engine.map(CoreBridge.init(engine:))
        activePollNs = activePollMs * 1_000_000
        idlePollNs = idlePollMs * 1_000_000
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
        } catch {
            errorMessage = Self.describe(error)
        }
        await refreshResults()
    }

    // MARK: Results

    /// Fire-and-forget refresh for view callbacks (typing, selection).
    func scheduleRefresh() {
        Task { await refreshResults() }
    }

    /// Runs the current sidebar selection + search text off the main actor
    /// and applies the result unless a newer request superseded it.
    func refreshResults() async {
        guard let bridge else { return }
        generation += 1
        let mine = generation
        let selection = sidebarSelection
        let text = searchText
        do {
            let newRows: [FileRow]
            let newTotal: UInt64
            switch selection {
            case .collection(let id):
                newRows = try await bridge.collectionRows(id: id)
                newTotal = UInt64(newRows.count)
            case .allSamples, .favorites, .library:
                var filters = SearchFilters()
                if selection == .favorites { filters.favoritesOnly = true }
                if case .library(let id) = selection { filters.libraryIds = [id] }
                let page = try await bridge.search(
                    SearchQuery(text: text, filters: filters, limit: Self.pageLimit))
                newRows = page.rows
                newTotal = page.total
            }
            guard mine == generation else { return }  // a newer request exists
            rows = newRows
            total = newTotal
            if let id = selectedFileId, !newRows.contains(where: { $0.fileId == id }) {
                selectedFileId = nil
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
            Task {
                await reloadSidebar()
                await refreshResults()
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
