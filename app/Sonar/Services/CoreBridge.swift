import Foundation

/// Async facade over `SonarEngine` (ADR-032 threading rules). Every call that
/// may block (open, scan, search, remove, row fetches) runs on a detached
/// background task, so callers on the main actor never stall. Cheap calls
/// (`indexingProgress`, `pollEvents`, pause/resume) are exposed synchronously.
/// No product logic lives here: it only forwards to the Rust core.
struct CoreBridge: Sendable {
    /// The underlying engine (`Sendable`: any thread may call it).
    let engine: SonarEngine

    init(engine: SonarEngine) { self.engine = engine }

    /// Opens the engine in the default home (`SONAR_HOME` is honoured by the
    /// core) off the main thread.
    static func openDefault() async throws -> CoreBridge {
        let engine = try await Task.detached { try SonarEngine.openDefault() }.value
        return CoreBridge(engine: engine)
    }

    // MARK: Blocking calls (off the main actor)

    func libraries() async throws -> [LibraryInfo] {
        let engine = engine
        return try await Task.detached { try engine.libraries() }.value
    }

    func collections() async throws -> [CollectionInfo] {
        let engine = engine
        return try await Task.detached { try engine.collections() }.value
    }

    /// Adds the folder, scans it, starts watching and starts indexing.
    func addLibrary(path: String, includeEmbeddings: Bool) async throws -> AddedLibrary {
        let engine = engine
        return try await Task.detached {
            let added = try engine.addLibrary(path: path)
            try engine.startWatching()
            _ = try engine.startIndexing(includeEmbeddings: includeEmbeddings)
            return added
        }.value
    }

    func removeLibrary(id: Int64) async throws -> Bool {
        let engine = engine
        return try await Task.detached { try engine.removeLibrary(libraryId: id) }.value
    }

    func search(_ query: SearchQuery) async throws -> SearchPage {
        let engine = engine
        return try await Task.detached { try engine.search(query: query) }.value
    }

    /// Neighbours of a seed (shares the search cancel flag: a newer
    /// search/similar call makes this one throw `Cancelled`).
    func similarTo(_ query: SimilarQuery) async throws -> SimilarPage {
        let engine = engine
        return try await Task.detached { try engine.similarTo(query: query) }.value
    }

    func collectionRows(id: Int64) async throws -> [FileRow] {
        let engine = engine
        return try await Task.detached { try engine.collectionRows(id: id) }.value
    }

    func setFavorite(fileIds: [Int64], on: Bool) async throws {
        let engine = engine
        _ = try await Task.detached { try engine.setFavorite(fileIds: fileIds, on: on) }.value
    }

    func startWatching() async throws {
        let engine = engine
        try await Task.detached { try engine.startWatching() }.value
    }

    // MARK: Cheap calls

    /// Starts (or continues, if one is running) the indexing job.
    func startIndexing(includeEmbeddings: Bool) throws {
        _ = try engine.startIndexing(includeEmbeddings: includeEmbeddings)
    }
    func indexingProgress() -> IndexingProgress { engine.indexingProgress() }
    func pollEvents() -> [EngineEvent] { engine.pollEvents() }
    func pauseIndexing() { engine.pauseIndexing() }
    func resumeIndexing() { engine.resumeIndexing() }
}
