import Foundation

/// Similarity mode of the app model: entry points, slider changes and the
/// query (all ranking happens in the core).
extension AppModel {
    /// Results to show for at most this many neighbours (single page).
    static let similarLimit: UInt32 = 200

    /// "Find similar" from a library row (also re-seeds from similarity results).
    func findSimilar(to row: FileRow) {
        startSimilarity(.file(id: row.fileId, name: row.name))
    }

    /// "Find similar" from a file outside the library (drop).
    func findSimilar(externalFile url: URL) {
        startSimilarity(.external(url: url))
    }

    /// Cmd-Shift-F: the selected row is the seed.
    func findSimilarToSelection() {
        if let row = selectedRow { findSimilar(to: row) }
    }

    private func startSimilarity(_ target: SimilarityTarget) {
        similarity.begin(target)
        scheduleRefresh()
    }

    /// Leaves similarity mode and restores the text search.
    func exitSimilarity() {
        guard similarity.isActive else { return }
        similarity.exit()
        scheduleRefresh()
    }

    /// Typing in the search field leaves similarity mode.
    func searchTextChanged() {
        similarity.exit()
        scheduleRefresh()
    }

    func setSimilarityWeight(_ aspect: SimilarityAspect, _ value: Float) {
        guard similarity.sliders[aspect] != min(max(value, 0), 1) else { return }
        similarity.sliders[aspect] = value
        scheduleSimilarRefresh()
    }

    func resetSimilarityWeights() {
        guard similarity.sliders != AspectSliders() else { return }
        similarity.sliders = AspectSliders()
        scheduleSimilarRefresh()
    }

    /// Debounced re-query; only the newest slider change runs.
    private func scheduleSimilarRefresh() {
        let token = similarity.nextDebounceToken()
        let ns = similarity.debounceNs
        track {
            if ns > 0 { try? await Task.sleep(nanoseconds: ns) }
            guard token == self.similarity.debounceToken else { return }
            await self.refreshResults(keepScroll: false)
        }
    }

    /// Runs the similarity query off the main actor and shows its rows
    /// (nearest first) unless a newer request superseded it.
    func refreshSimilar(keepScroll: Bool = false) async {
        guard let target = similarity.target else { return }
        let search: @Sendable (SimilarQuery) async throws -> SimilarPage
        if let override = similarity.searchOverride {
            search = override
        } else if let bridge {
            search = { try await bridge.similarTo($0) }
        } else {
            return
        }
        generation += 1
        let mine = generation
        similarity.markLoading()
        do {
            // Park the pager on an in-memory loader first: a page fetch of the
            // old text loader would call `engine.search` and cancel our query.
            guard try await pager.reset(loader: ArrayResultLoader(rows: []), preservePosition: keepScroll),
                mine == generation
            else { return }
            var base = SearchFilters()
            if sidebarSelection == .favorites { base.favoritesOnly = true }
            if case .library(let id) = sidebarSelection { base.libraryIds = [id] }
            let query = SimilarQuery(
                seed: target.seed, weights: try similarity.sliders.weights(),
                filters: filters.apply(to: base), limit: Self.similarLimit)
            let page = try await Self.searchRetryingCancelled(query, search) { mine == self.generation }
            guard mine == generation else { return }
            guard let page else {
                similarity.finish(hits: [])  // still cancelled after retries: never stick on the spinner
                return
            }
            let applied = try await pager.reset(
                loader: ArrayResultLoader(rows: page.rows), preservePosition: keepScroll)
            guard applied, mine == generation else { return }
            similarity.finish(hits: page.hits)
            if page.rows.isEmpty {
                select(nil)
            } else if let id = selectedFileId, let fresh = pager.cachedRow(fileId: id) {
                updateSelectedSnapshot(fresh)
            }
        } catch {
            guard mine == generation else { return }
            _ = try? await pager.reset(loader: ArrayResultLoader(rows: []), preservePosition: false)
            guard mine == generation else { return }
            similarity.fail(error)
        }
    }

    /// Runs `search`; a `Cancelled` while this request is still the newest
    /// (another core search stole the cancel flag) is retried up to 3 times.
    /// Returns nil when still cancelled after the retries.
    private static func searchRetryingCancelled(
        _ query: SimilarQuery, _ search: @Sendable (SimilarQuery) async throws -> SimilarPage,
        isCurrent: @MainActor () -> Bool
    ) async throws -> SimilarPage? {
        var attempts = 0
        while true {
            do {
                return try await search(query)
            } catch FfiError.Cancelled {
                attempts += 1
                let current = await isCurrent()
                if attempts > 3 || !current { return nil }
            }
        }
    }
}
