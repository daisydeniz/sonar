import Foundation

/// Duplicate-finder calls (ADR-036). `duplicates` blocks (hashing on the
/// first run), so it runs off the main actor; `cancelDuplicates` is cheap.
extension CoreBridge {
    func duplicates(_ query: DuplicatesQuery) async throws -> DuplicatesResult {
        let engine = engine
        return try await Task.detached { try engine.duplicates(query: query) }.value
    }

    /// Makes a running `duplicates` call throw `FfiError.Cancelled`.
    func cancelDuplicates() { engine.cancelDuplicates() }

    /// Rows of the given files, in the order asked (unknown ids skipped).
    func rows(fileIds: [Int64]) async throws -> [FileRow] {
        let engine = engine
        return try await Task.detached { try engine.rows(fileIds: fileIds) }.value
    }
}
