import XCTest
@testable import Sonar

@MainActor
final class ResultsPagerTests: XCTestCase {
    func testPlaceholderThenLoad() async throws {
        let pager = ResultsPager(pageSize: 100)
        let loader = SyntheticPageLoader(rowCount: 10_000)
        let applied = try await pager.reset(loader: loader)
        XCTAssertTrue(applied)
        XCTAssertEqual(pager.count, 10_000)
        XCTAssertEqual(pager.row(at: 5)?.name, "row 5", "first page is ready after reset")

        var changes: [ResultsPager.Change] = []
        pager.onChange = { changes.append($0) }
        XCTAssertNil(pager.row(at: 5_050), "an unloaded row is a placeholder")
        let loaded = await waitFor { pager.peek(5_050) != nil }
        XCTAssertTrue(loaded)
        XCTAssertEqual(pager.peek(5_050)?.name, "row 5050")
        XCTAssertTrue(changes.contains {
            if case .pagesLoaded(let r) = $0 { return r == 5_000..<5_100 }
            return false
        })
    }

    func testLRUEvictionBoundsTheCache() async throws {
        let pager = ResultsPager(pageSize: 50, maxCachedPages: 4)
        try await pager.reset(loader: SyntheticPageLoader(rowCount: 100_000))
        for page in [10, 20, 30, 40, 50, 60, 70] {
            _ = pager.row(at: page * 50)
            let ok = await waitFor { pager.peek(page * 50) != nil }
            XCTAssertTrue(ok)
            XCTAssertLessThanOrEqual(pager.cachedPageCount, 4)
        }
        XCTAssertNil(pager.peek(0), "the oldest page was evicted")
        XCTAssertNotNil(pager.peek(70 * 50), "the newest page is cached")
    }

    func testResetDropsOldGenerationResults() async throws {
        let pager = ResultsPager(pageSize: 100)
        let old = SyntheticPageLoader(rowCount: 10_000, prefix: "old", delayNs: 150_000_000)
        try await pager.reset(loader: old)
        _ = pager.row(at: 3_000)  // slow page of the old query, in flight
        let ok = await waitFor { old.requestedOffsets.contains(3_000) }
        XCTAssertTrue(ok)
        try await pager.reset(loader: SyntheticPageLoader(rowCount: 500, prefix: "new"))
        XCTAssertEqual(pager.count, 500)
        try await Task.sleep(nanoseconds: 300_000_000)  // let the old page arrive
        XCTAssertEqual(pager.count, 500)
        XCTAssertNil(pager.peek(3_000))
        XCTAssertEqual(pager.peek(0)?.name, "new 0")
    }

    func testSupersededResetReturnsFalse() async throws {
        let pager = ResultsPager(pageSize: 100)
        let slowFirst = SlowFirstPageLoader()
        let first = Task { try await pager.reset(loader: slowFirst) }
        await Task.yield()
        let second = try await pager.reset(loader: SyntheticPageLoader(rowCount: 10, prefix: "b"))
        let firstApplied = try await first.value
        XCTAssertTrue(second)
        XCTAssertFalse(firstApplied)
        XCTAssertEqual(pager.peek(0)?.name, "b 0")
    }

    func testShortFinalPageCorrectsTheCount() async throws {
        let pager = ResultsPager(pageSize: 200)
        // The core claims 1000 matches, but only 450 rows exist.
        try await pager.reset(loader: SyntheticPageLoader(rowCount: 450, claimedTotal: 1_000))
        XCTAssertEqual(pager.count, 1_000)
        _ = pager.row(at: 450)  // page 2: 50 rows
        let ok = await waitFor { pager.count == 450 }
        XCTAssertTrue(ok, "count is \(pager.count)")
    }

    func testSortChangeResetsWithTheNewKey() async throws {
        let pager = ResultsPager(pageSize: 100)
        let controller = ResultsTableController(pager: pager)
        var sorts: [(SortKey, Bool)] = []
        controller.onSort = { sorts.append(($0, $1)) }
        // Setting the descriptors fires the delegate, as a header click does.
        controller.tableView.sortDescriptors = [NSSortDescriptor(key: "bpm", ascending: false)]
        XCTAssertEqual(sorts.count, 1)
        XCTAssertEqual(sorts.first?.0, .bpm)
        XCTAssertEqual(sorts.first?.1, true)
        controller.tableView.sortDescriptors = [NSSortDescriptor(key: "library", ascending: true)]
        XCTAssertEqual(sorts.last?.0, .library)
        XCTAssertEqual(sorts.last?.1, false)
    }
}

/// First page takes a while (so a second reset can overtake it).
private struct SlowFirstPageLoader: ResultPageLoader {
    func loadPage(offset: Int, limit: Int) async throws -> PageResult {
        try await Task.sleep(nanoseconds: 100_000_000)
        return PageResult(rows: [], total: 0, hasMore: false)
    }
}
