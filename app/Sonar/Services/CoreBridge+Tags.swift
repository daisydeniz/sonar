import Foundation

/// Tag management calls (T12.8, DESIGN §4.12 layer 3). Blocking in the core,
/// so they run off the main actor like every other bridge call.
extension CoreBridge {
    private func runTags<T: Sendable>(_ body: @escaping @Sendable (SonarEngine) throws -> T) async throws -> T {
        let engine = engine
        return try await Task.detached { try body(engine) }.value
    }

    /// Renames a tag (onto an existing one it merges). Returns the affected file ids.
    func renameTag(from: String, to: String) async throws -> [Int64] {
        try await runTags { try $0.renameTag(from: from, to: to) }
    }

    /// Merges `sources` into `target`. Returns the affected file ids.
    func mergeTags(sources: [String], target: String) async throws -> [Int64] {
        try await runTags { try $0.mergeTags(sources: sources, target: target) }
    }

    /// Deletes a tag from every file. Returns the ids that carried it.
    func deleteTag(name: String) async throws -> [Int64] {
        try await runTags { try $0.deleteTag(name: name) }
    }

    /// Tags that look like the same word (ignored pairs left out).
    func similarTags() async throws -> [SimilarTagPair] { try await runTags { try $0.similarTags() } }

    /// Never suggest this pair again.
    func ignoreSimilarTags(a: String, b: String) async throws {
        try await runTags { try $0.ignoreSimilarTags(a: a, b: b) }
    }

    /// Ids of every file (hidden ones too) that carries `tag`, paged from the
    /// core's own tag filter. Used to snapshot what a rename/merge touches, so
    /// Undo can restore it. A page cancelled by a newer search is retried.
    func fileIds(withTag tag: String) async throws -> [Int64] {
        try await Self.pagedFileIds { offset, limit in
            var filters = SearchFilters()
            filters.tags = [tag]
            filters.includeHidden = true
            let query = SearchQuery(text: "", filters: filters, offset: offset, limit: limit)
            for _ in 0..<5 {
                do { return try await self.search(query) } catch FfiError.Cancelled { continue }
            }
            throw FfiError.Cancelled
        }
    }

    /// Largest page the FFI `search` returns (`MAX_PAGE` in `sonar-ffi`).
    static let searchPageLimit: UInt32 = 1000

    /// Collects every row id by paging `fetch(offset, limit)`. Advances by the
    /// rows actually returned, since the core may cap a page below `limit`.
    static func pagedFileIds(
        _ fetch: @Sendable (UInt32, UInt32) async throws -> SearchPage
    ) async throws -> [Int64] {
        var ids: [Int64] = []
        var offset: UInt32 = 0
        while true {
            let page = try await fetch(offset, searchPageLimit)
            ids += page.rows.map(\.fileId)
            guard page.hasMore, !page.rows.isEmpty else { return ids }
            offset += UInt32(page.rows.count)
        }
    }
}
