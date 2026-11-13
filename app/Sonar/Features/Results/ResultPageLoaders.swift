import Foundation

/// Pages of one engine search: the same query at a different offset/limit.
/// Sorting stays in the core (`query.sort` / `query.descending`).
struct EngineResultLoader: ResultPageLoader {
    let bridge: CoreBridge
    let query: SearchQuery
    /// Ask the core for facet counts. Only page 0 requests them (a facet
    /// pass costs a collector run; further pages never repeat it).
    var wantsFacets = false

    /// The query for one page; facets only on the first page.
    func pageQuery(offset: Int, limit: Int) -> SearchQuery {
        var q = query
        q.offset = UInt32(offset)
        q.limit = UInt32(limit)
        q.facets = wantsFacets && offset == 0
        return q
    }

    func loadPage(offset: Int, limit: Int) async throws -> PageResult {
        let page = try await bridge.search(pageQuery(offset: offset, limit: limit))
        return PageResult(rows: page.rows, total: Int(page.total), hasMore: page.hasMore, facets: page.facets)
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
