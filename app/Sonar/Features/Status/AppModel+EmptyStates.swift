import Foundation

/// Why the results area has nothing to show (T7.2).
enum EmptyStateKind: Equatable, Sendable {
    /// A query or filter matched nothing; offers "Clear Search & Filters".
    case noResults
    case noFavorites
    case emptyCollection
    case emptyLibrary
    /// Similarity found no neighbours (`analyzing`: indexing still runs).
    case noSimilar(analyzing: Bool)
}

extension AppModel {
    /// The empty state to show, or nil while loading, with rows, or in views
    /// that have their own (duplicates, onboarding).
    var emptyState: EmptyStateKind? {
        guard engineState == .ready, !showsOnboarding, sidebarSelection != .duplicates,
            resultCount == 0
        else { return nil }
        if similarity.isActive {
            return similarity.isLoading ? nil : .noSimilar(analyzing: progress.running)
        }
        if isLoadingResults { return nil }
        if case .collection = sidebarSelection { return .emptyCollection }
        if hasQueryOrFilters { return .noResults }
        switch sidebarSelection {
        case .favorites: return .noFavorites
        default: return .emptyLibrary
        }
    }

    /// A search text or any filter chip is set.
    var hasQueryOrFilters: Bool {
        !searchText.trimmingCharacters(in: .whitespaces).isEmpty || filters.isActive
    }

    /// "Clear Search & Filters" action of the no-results state.
    func clearSearchAndFilters() {
        searchText = ""
        filters = FilterState()
    }
}
