import Foundation

/// The user's filter-chip selection. Pure value type: it only describes the
/// filters; the Rust core does the filtering and the counting (no Swift-side
/// filtering). `apply(to:)` merges it into the `SearchFilters` of a query.
struct FilterState: Equatable, Sendable {
    var categories: Set<String> = []
    var kinds: Set<String> = []
    var keys: Set<String> = []
    /// Also match keys compatible with the selected ones (core-side).
    var compatibleKeys = false
    var tags: Set<String> = []
    /// Libraries chosen with chips (any-of); ignored while the sidebar
    /// selects a single library.
    var libraryIds: Set<Int64> = []
    var favoritesOnly = false
    var bpmMin: Double?
    var bpmMax: Double?
    /// Duration bounds in seconds (the core takes milliseconds).
    var durationMinSec: Double?
    var durationMaxSec: Double?

    /// True when any filter is set.
    var isActive: Bool { self != FilterState() }

    /// Number of active filter groups (for accessibility / badges).
    var activeCount: Int {
        [!categories.isEmpty, !kinds.isEmpty, !keys.isEmpty, !tags.isEmpty, !libraryIds.isEmpty,
         favoritesOnly, bpmMin != nil || bpmMax != nil,
         durationMinSec != nil || durationMaxSec != nil].filter { $0 }.count
    }

    /// Adds or removes `value` in the set.
    static func toggle<T: Hashable>(_ value: T, in set: inout Set<T>) {
        if !set.insert(value).inserted { set.remove(value) }
    }

    /// Merges the chips into `base` (which carries the sidebar's own
    /// library / favourites restriction): sidebar AND chips.
    func apply(to base: SearchFilters) -> SearchFilters {
        var f = base
        f.categories = categories.sorted()
        f.kinds = kinds.sorted()
        f.keys = keys.sorted()
        f.compatibleKeys = compatibleKeys && !keys.isEmpty
        f.tags = tags.sorted()
        if base.libraryIds.isEmpty { f.libraryIds = libraryIds.sorted() }
        f.favoritesOnly = base.favoritesOnly || favoritesOnly
        f.bpmMin = bpmMin.flatMap { $0.isFinite ? $0 : nil }
        f.bpmMax = bpmMax.flatMap { $0.isFinite ? $0 : nil }
        f.durationMinMs = durationMinSec.flatMap(Self.milliseconds)
        f.durationMaxMs = durationMaxSec.flatMap(Self.milliseconds)
        return f
    }

    /// Seconds to milliseconds: non-finite input is dropped (no bound),
    /// negatives become 0 and huge values are clamped (no trap).
    static func milliseconds(_ seconds: Double) -> UInt64? {
        guard seconds.isFinite else { return nil }
        return UInt64(min(max(0, seconds), 1e12) * 1000)
    }
}

/// A chip to render: a facet value with its count, or a selected value
/// that the facets no longer list (count 0, kept visible).
struct FacetChip: Identifiable, Equatable, Sendable {
    let value: String
    let count: UInt64
    let selected: Bool
    var id: String { value }

    /// Chips for one dimension: every facet value (counts from the core), plus
    /// selected values missing from the facets with count 0.
    static func chips(facets: [FacetCount], selected: Set<String>) -> [FacetChip] {
        var chips = facets.map { FacetChip(value: $0.value, count: $0.count, selected: selected.contains($0.value)) }
        let listed = Set(facets.map(\.value))
        for value in selected.subtracting(listed).sorted() {
            chips.append(FacetChip(value: value, count: 0, selected: true))
        }
        return chips
    }
}
