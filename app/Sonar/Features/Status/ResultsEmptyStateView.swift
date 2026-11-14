import SwiftUI

/// Renders an `EmptyStateKind` with the shared `EmptyStateView`.
@MainActor
struct ResultsEmptyStateView: View {
    let model: AppModel
    let kind: EmptyStateKind

    var body: some View {
        switch kind {
        case .noResults:
            EmptyStateView(
                symbol: "magnifyingglass", title: "No results",
                message: "Nothing matches the current search and filters.",
                actionTitle: "Clear Search & Filters") { model.clearSearchAndFilters() }
        case .noFavorites:
            EmptyStateView(
                symbol: "heart", title: "No favorites yet",
                message: "Press Return on a sample or use Favorite (⌘L) to keep it here.")
        case .emptyCollection:
            EmptyStateView(
                symbol: "square.stack", title: "Empty collection",
                message: "Add samples from the results with the right-click menu.")
        case .emptyLibrary:
            EmptyStateView(
                symbol: "folder", title: "No samples here",
                message: "No audio files found yet. They appear as the folder is scanned.")
        case .noSimilar(let analyzing):
            EmptyStateView(
                symbol: "waveform.badge.magnifyingglass", title: "No similar samples",
                message: analyzing
                    ? "Sonar is still analyzing your samples. Try again when indexing finishes."
                    : "Nothing matches. Try clearing the filters or raising a slider.")
        }
    }
}
