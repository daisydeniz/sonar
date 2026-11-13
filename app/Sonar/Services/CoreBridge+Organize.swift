import Foundation

/// Organize calls (favorites, hide, tags, collections, overrides). All are
/// synchronous in the core (~90 ms on a big index), so they run off the main
/// actor like every other blocking bridge call.
extension CoreBridge {
    private func run<T: Sendable>(_ body: @escaping @Sendable (SonarEngine) throws -> T) async throws -> T {
        let engine = engine
        return try await Task.detached { try body(engine) }.value
    }

    func setHidden(fileIds: [Int64], on: Bool) async throws {
        _ = try await run { try $0.setHidden(fileIds: fileIds, on: on) }
    }

    func addTags(fileIds: [Int64], tags: [String]) async throws {
        _ = try await run { try $0.addTags(fileIds: fileIds, tags: tags) }
    }

    func removeTags(fileIds: [Int64], tags: [String]) async throws {
        _ = try await run { try $0.removeTags(fileIds: fileIds, tags: tags) }
    }

    func allTags() async throws -> [TagInfo] { try await run { try $0.tags() } }

    func setOverride(fileIds: [Int64], field: OverrideField, value: String) async throws {
        _ = try await run { try $0.setOverride(fileIds: fileIds, field: field, value: value) }
    }

    func clearOverride(fileIds: [Int64], field: OverrideField) async throws {
        _ = try await run { try $0.clearOverride(fileIds: fileIds, field: field) }
    }

    func createCollection(name: String) async throws -> Int64 {
        try await run { try $0.createCollection(name: name) }
    }

    func renameCollection(id: Int64, name: String) async throws {
        try await run { try $0.renameCollection(id: id, name: name) }
    }

    func deleteCollection(id: Int64) async throws {
        _ = try await run { try $0.deleteCollection(id: id) }
    }

    func addToCollection(id: Int64, fileIds: [Int64]) async throws {
        _ = try await run { try $0.addToCollection(id: id, fileIds: fileIds) }
    }

    func removeFromCollection(id: Int64, fileIds: [Int64]) async throws {
        _ = try await run { try $0.removeFromCollection(id: id, fileIds: fileIds) }
    }
}
