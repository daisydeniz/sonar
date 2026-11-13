import Foundation
@testable import Sonar

/// Deterministic in-memory `ResultPageLoader` for pager and table tests.
/// Row `i` is named `"<prefix> <i>"` with `fileId == i`.
final class SyntheticPageLoader: ResultPageLoader, @unchecked Sendable {
    /// Rows that really exist.
    let rowCount: Int
    /// What the first page claims as `total` (the core's estimate may be off).
    let claimedTotal: Int
    let prefix: String
    /// Pages other than the first take this long (0 = immediate).
    let delayNs: UInt64

    private let lock = NSLock()
    private var offsets: [Int] = []

    init(rowCount: Int, claimedTotal: Int? = nil, prefix: String = "row", delayNs: UInt64 = 0) {
        self.rowCount = rowCount
        self.claimedTotal = claimedTotal ?? rowCount
        self.prefix = prefix
        self.delayNs = delayNs
    }

    /// Offsets requested so far, in order.
    var requestedOffsets: [Int] { lock.withLock { offsets } }

    func loadPage(offset: Int, limit: Int) async throws -> PageResult {
        lock.withLock { offsets.append(offset) }
        if delayNs > 0, offset > 0 { try await Task.sleep(nanoseconds: delayNs) }
        let end = min(offset + limit, rowCount)
        let rows = offset < end ? (offset..<end).map { makeRow($0) } : []
        return PageResult(rows: rows, total: claimedTotal, hasMore: rows.count == limit)
    }

    private func makeRow(_ i: Int) -> FileRow {
        FileRow(
            fileId: Int64(i), libraryId: 1, path: "/synthetic/\(prefix)\(i).wav", name: "\(prefix) \(i)",
            folders: "synthetic", category: "kick", kind: i % 2 == 0 ? "loop" : "one_shot",
            bpm: Double(60 + i % 120), key: "Am", durationMs: UInt64(500 + i % 4000),
            sampleRate: 44100, channels: 2, favorite: false, hidden: false, tags: [])
    }
}

/// Polls `condition` on the main actor until it holds or `timeout` passes.
@MainActor
func waitFor(timeout: TimeInterval = 5, _ condition: () -> Bool) async -> Bool {
    let deadline = Date().addingTimeInterval(timeout)
    while !condition() {
        if Date() >= deadline { return false }
        try? await Task.sleep(nanoseconds: 2_000_000)
    }
    return true
}
