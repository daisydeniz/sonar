import Foundation
import Observation

/// One page of results as delivered by a `ResultPageLoader`.
struct PageResult: Sendable {
    var rows: [FileRow]
    /// Matches reported by the core (may be off in hybrid mode; the pager
    /// corrects its row count from short pages and `hasMore`).
    var total: Int
    /// The page was full, so more rows may follow.
    var hasMore: Bool
    /// Facet counts (only when the loader asked for them on page 0).
    var facets: SearchFacets? = nil
}

/// Source of result pages (the engine in the app, a synthetic one in tests).
protocol ResultPageLoader: Sendable {
    /// Rows `offset ..< offset + limit` of the current query.
    func loadPage(offset: Int, limit: Int) async throws -> PageResult
}

/// Lazy, bounded page cache behind the results table (ARCHITECTURE §8).
///
/// - `row(at:)` returns the cached row or `nil` (placeholder) and schedules a
///   page fetch; the table is told which rows changed via `onChange`.
/// - Page fetches are serialized (one in flight, newest request first): the
///   core cancels a running search when another starts, so concurrent page
///   loads would cancel each other. A page that still comes back `Cancelled`
///   while its generation is current is retried up to `maxRetries` times.
/// - Every `reset(loader:)` (new query or sort) starts a new generation;
///   results of older generations are dropped.
/// - The cache is an LRU of at most `maxCachedPages` pages, so 1M rows never
///   sit in memory at once.
@MainActor
@Observable
final class ResultsPager {
    /// What changed, for the table.
    enum Change {
        /// New query: reload everything (`preservePosition`: keep scroll).
        case reset(preservePosition: Bool)
        /// The row count changed (estimate corrected).
        case countChanged
        /// Rows in this range were loaded (reload them if visible).
        case pagesLoaded(Range<Int>)
    }

    static let defaultPageSize = 200

    /// Rows in the result list (estimated until the last page is seen).
    private(set) var count = 0
    /// Facet counts of the current query (from its first page; nil if none).
    private(set) var facets: SearchFacets?

    @ObservationIgnored let pageSize: Int
    @ObservationIgnored let maxCachedPages: Int
    @ObservationIgnored let maxRetries: Int
    @ObservationIgnored var onChange: ((Change) -> Void)?
    /// Called with non-cancellation page-load failures of the current query.
    @ObservationIgnored var onError: ((Error) -> Void)?

    private final class Page {
        let rows: [FileRow]
        var lastUse: Int
        init(rows: [FileRow], lastUse: Int) { self.rows = rows; self.lastUse = lastUse }
    }

    @ObservationIgnored private var cache: [Int: Page] = [:]
    @ObservationIgnored private var tick = 0
    @ObservationIgnored private var generation = 0
    @ObservationIgnored private var loader: ResultPageLoader?
    @ObservationIgnored private var resetting = false
    @ObservationIgnored private var pending: [Int] = []
    @ObservationIgnored private var inFlight: Int?
    @ObservationIgnored private var failed: Set<Int> = []
    @ObservationIgnored private var runner: Task<Void, Never>?
    private let maxPending = 6

    init(pageSize: Int = ResultsPager.defaultPageSize, maxCachedPages: Int = 64, maxRetries: Int = 3) {
        self.pageSize = pageSize
        self.maxCachedPages = maxCachedPages
        self.maxRetries = maxRetries
    }

    /// Number of pages currently cached.
    var cachedPageCount: Int { cache.count }

    // MARK: Reading

    /// The row at `index`, or `nil` while its page is loading. Triggers the
    /// fetch (and a prefetch of the neighbouring page near a page edge).
    func row(at index: Int) -> FileRow? {
        guard index >= 0, index < count else { return nil }
        let p = index / pageSize
        let r = index % pageSize
        guard let page = cache[p] else {
            request(p)
            return nil
        }
        tick += 1
        page.lastUse = tick
        if r >= pageSize * 3 / 4 { prefetch(p + 1) } else if r < pageSize / 4 { prefetch(p - 1) }
        return r < page.rows.count ? page.rows[r] : nil
    }

    /// Like `row(at:)` but without side effects (no fetch, no LRU touch).
    func peek(_ index: Int) -> FileRow? {
        guard index >= 0, let page = cache[index / pageSize] else { return nil }
        let r = index % pageSize
        return r < page.rows.count ? page.rows[r] : nil
    }

    /// Index of a cached row with this file id.
    func index(ofFileId id: Int64) -> Int? {
        for (p, page) in cache {
            if let i = page.rows.firstIndex(where: { $0.fileId == id }) { return p * pageSize + i }
        }
        return nil
    }

    /// A cached row with this file id.
    func cachedRow(fileId id: Int64) -> FileRow? {
        index(ofFileId: id).flatMap { peek($0) }
    }

    // MARK: Reset

    /// Starts a new generation with `loader` and applies its first page
    /// (the old rows stay visible until then). Returns false when the result
    /// was superseded by a newer reset (or kept being cancelled); throws on a
    /// real load error of the current generation.
    @discardableResult
    func reset(loader: ResultPageLoader, preservePosition: Bool = false) async throws -> Bool {
        generation += 1
        let gen = generation
        resetting = true
        pending.removeAll()
        inFlight = nil
        runner = nil  // an older runner stops at its next generation check
        do {
            guard let result = try await fetch(loader, page: 0, generation: gen), gen == generation
            else { return false }
            cache.removeAll()
            failed.removeAll()
            self.loader = loader
            resetting = false
            store(page: 0, result: result)
            facets = result.facets
            count = 0  // so the first countAfter() sets it from page 0
            count = countAfter(page: 0, result: result)
            onChange?(.reset(preservePosition: preservePosition))
            return true
        } catch {
            if gen == generation { resetting = false }
            throw error
        }
    }

    // MARK: Fetching

    private func request(_ p: Int) {
        guard !resetting, loader != nil, p >= 0, p * pageSize < count,
            cache[p] == nil, p != inFlight, !failed.contains(p), !pending.contains(p)
        else { return }
        pending.append(p)
        if pending.count > maxPending { pending.removeFirst() }  // fast scroll: drop the stalest
        startRunner()
    }

    private func prefetch(_ p: Int) {
        guard p >= 0, p * pageSize < count, cache[p] == nil else { return }
        guard !pending.contains(p), p != inFlight else { return }
        // Insert below the page being requested: the runner pops the newest first.
        pending.insert(p, at: 0)
        if pending.count > maxPending { pending.removeFirst() }
        startRunner()
    }

    private func startRunner() {
        guard runner == nil, let loader, !resetting else { return }
        let gen = generation
        runner = Task { [weak self] in await self?.drain(generation: gen, loader: loader) }
    }

    private func drain(generation gen: Int, loader: ResultPageLoader) async {
        while gen == generation, let p = pending.popLast() {
            inFlight = p
            do {
                if let result = try await fetch(loader, page: p, generation: gen), gen == generation {
                    store(page: p, result: result)
                    let countChanged = updateCount(page: p, result: result)
                    if countChanged { onChange?(.countChanged) }
                    onChange?(.pagesLoaded(p * pageSize..<p * pageSize + result.rows.count))
                }
            } catch {
                guard gen == generation else { break }
                failed.insert(p)
                onError?(error)
            }
        }
        if gen == generation {
            inFlight = nil
            runner = nil
        }
    }

    /// Loads a page, retrying `Cancelled` while `generation` is current.
    /// Returns nil when superseded or when retries ran out.
    private func fetch(_ loader: ResultPageLoader, page p: Int, generation gen: Int) async throws
        -> PageResult?
    {
        var attempts = 0
        while true {
            do {
                return try await loader.loadPage(offset: p * pageSize, limit: pageSize)
            } catch where Self.isCancellation(error) {
                attempts += 1
                if gen != generation || attempts >= maxRetries { return nil }
                await Task.yield()
            }
        }
    }

    private static func isCancellation(_ error: Error) -> Bool {
        if error is CancellationError { return true }
        if case FfiError.Cancelled = error { return true }
        return false
    }

    // MARK: Cache + count

    private func store(page p: Int, result: PageResult) {
        tick += 1
        cache[p] = Page(rows: result.rows, lastUse: tick)
        while cache.count > maxCachedPages, let victim = cache.min(by: { $0.value.lastUse < $1.value.lastUse }) {
            cache.removeValue(forKey: victim.key)
        }
    }

    private func countAfter(page p: Int, result: PageResult) -> Int {
        let base = p * pageSize
        if result.rows.count < pageSize || !result.hasMore { return base + result.rows.count }
        let atLeast = base + result.rows.count + 1  // hasMore: at least one more row
        return p == 0 ? max(result.total, atLeast) : max(count, atLeast)
    }

    private func updateCount(page p: Int, result: PageResult) -> Bool {
        let new = countAfter(page: p, result: result)
        guard new != count else { return false }
        count = new
        return true
    }
}
