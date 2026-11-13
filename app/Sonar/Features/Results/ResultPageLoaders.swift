import Foundation

/// Pages of one engine search: the same query at a different offset/limit.
/// Sorting stays in the core (`query.sort` / `query.descending`).
struct EngineResultLoader: ResultPageLoader {
    let bridge: CoreBridge
    let query: SearchQuery

    func loadPage(offset: Int, limit: Int) async throws -> PageResult {
        var q = query
        q.offset = UInt32(offset)
        q.limit = UInt32(limit)
        let page = try await bridge.search(q)
        return PageResult(rows: page.rows, total: Int(page.total), hasMore: page.hasMore)
    }
}

/// Pages over rows that are already in memory (collection contents).
struct ArrayResultLoader: ResultPageLoader {
    let rows: [FileRow]

    func loadPage(offset: Int, limit: Int) async throws -> PageResult {
        let start = min(offset, rows.count)
        let end = min(start + limit, rows.count)
        return PageResult(rows: Array(rows[start..<end]), total: rows.count, hasMore: end < rows.count)
    }
}
