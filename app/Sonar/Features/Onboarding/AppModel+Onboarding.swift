import Foundation

extension AppModel {
    /// First-run state: the engine is up, the sidebar loaded, no library yet.
    /// Derived (nothing persisted): adding a library ends it.
    var showsOnboarding: Bool {
        engineState == .ready && sidebarLoaded && libraries.isEmpty
    }

    /// Adds several folders one after another (onboarding, drops, panel).
    func addLibraries(paths: [String]) async {
        for path in paths { await addLibrary(path: path) }
    }

    /// Rescans one library, then refreshes sidebar and results.
    func rescanLibrary(id: Int64) async {
        guard let bridge else { return }
        do {
            _ = try await bridge.rescanLibrary(id: id)
        } catch {
            errorMessage = Self.describe(error)
        }
        await reloadSidebar()
        await refreshResults()
    }

    /// Whether any refresh is pending or none finished yet.
    var isLoadingResults: Bool { loadingRefreshes > 0 || !resultsLoadedOnce }
}

extension CoreBridge {
    /// Rescans the library folder off the main thread.
    func rescanLibrary(id: Int64) async throws -> ScanInfo {
        let engine = engine
        return try await Task.detached { try engine.rescanLibrary(libraryId: id) }.value
    }
}
