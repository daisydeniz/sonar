import Foundation

/// Mode choice of the panel's Key section. In `any`, a root selects both the
/// major and the minor key of that root.
enum KeyMode: String, CaseIterable, Identifiable, Sendable {
    case any, major, minor
    var id: String { rawValue }
    var title: String {
        switch self {
        case .any: return "Any"
        case .major: return "Major"
        case .minor: return "Minor"
        }
    }
}

/// A quick range chip of the Tempo / Duration sections: one (min, max) pair.
struct QuickRange: Equatable, Identifiable, Sendable {
    let min: Double?
    let max: Double?
    var id: String { "\(min.map { "\($0)" } ?? "")-\(max.map { "\($0)" } ?? "")" }
    func label(unit: String = "") -> String { FilterState.rangeSummary(min: min, max: max, unit: unit) ?? "" }
}

/// Category families of the panel (DESIGN §3): one hue each.
enum CategoryFamily: Int, CaseIterable, Sendable {
    case drums, percussion, bass, tonal, vocal, fx

    var title: String {
        switch self {
        case .drums: return "Drums"
        case .percussion: return "Percussion & metal"
        case .bass: return "Bass"
        case .tonal: return "Tonal"
        case .vocal: return "Vocal"
        case .fx: return "FX"
        }
    }

    static func of(_ category: String) -> CategoryFamily {
        switch Theme.categoryHue(category) {
        case .drum: return .drums
        case .metal: return .percussion
        case .bass: return .bass
        case .tonal: return .tonal
        case .vocal: return .vocal
        default: return .fx
        }
    }
}

/// Selection logic of the filter panel and the bar's active chips: pure
/// functions over `FilterState` / facet lists, no view code (unit tested).
/// Filtering and counting stay in the core.
enum FilterPanel {
    static let roots = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"]

    static let tempoRanges: [QuickRange] = [
        QuickRange(min: nil, max: 90), QuickRange(min: 90, max: 110), QuickRange(min: 110, max: 128),
        QuickRange(min: 128, max: 140), QuickRange(min: 140, max: 160), QuickRange(min: 160, max: nil),
    ]
    static let durationRanges: [QuickRange] = [
        QuickRange(min: nil, max: 0.5), QuickRange(min: 0.5, max: 1), QuickRange(min: 1, max: 2),
        QuickRange(min: 2, max: 5), QuickRange(min: 5, max: 10), QuickRange(min: 10, max: nil),
    ]

    // MARK: Category

    /// Category chips ordered by family, facet order (count) kept inside one.
    static func categoryChips(_ chips: [FacetChip]) -> [FacetChip] {
        chips.enumerated()
            .sorted { (CategoryFamily.of($0.element.value).rawValue, $0.offset) < (CategoryFamily.of($1.element.value).rawValue, $1.offset) }
            .map(\.element)
    }

    /// First `limit` chips (all when `expanded`), plus selected chips beyond
    /// the limit, so a selection is never hidden behind "Show More".
    static func visible(_ chips: [FacetChip], limit: Int, expanded: Bool) -> [FacetChip] {
        if expanded || chips.count <= limit { return chips }
        return chips.enumerated().filter { $0.offset < limit || $0.element.selected }.map(\.element)
    }

    // MARK: Key

    /// The stored key labels a root stands for in `mode` (`A` major, `Am` minor).
    static func keyLabels(root: String, mode: KeyMode) -> [String] {
        switch mode {
        case .any: return [root, root + "m"]
        case .major: return [root]
        case .minor: return [root + "m"]
        }
    }

    /// Selected when any of its labels is in `keys` (in Any: major or minor).
    static func isKeySelected(root: String, mode: KeyMode, in keys: Set<String>) -> Bool {
        keyLabels(root: root, mode: mode).contains { keys.contains($0) }
    }

    /// Toggles a root: selected → all its labels in `mode` removed, else added.
    static func toggleKey(root: String, mode: KeyMode, in keys: inout Set<String>) {
        let labels = keyLabels(root: root, mode: mode)
        if isKeySelected(root: root, mode: mode, in: keys) {
            keys.subtract(labels)
        } else {
            keys.formUnion(labels)
        }
    }

    /// Facet count of a root in Major / Minor; nil in Any (a mode-less key
    /// counts toward both modes, a sum would double-count).
    static func keyCount(root: String, mode: KeyMode, facets: [FacetCount]) -> Int? {
        guard mode != .any, let label = keyLabels(root: root, mode: mode).first else { return nil }
        return Int(facets.first { $0.value == label }?.count ?? 0)
    }

    /// A root with no key in the facets at all (any mode) and not selected
    /// is disabled; in Any mode counts are unknown, so only this check applies.
    static func isKeyAvailable(root: String, mode: KeyMode, facets: [FacetCount], keys: Set<String>) -> Bool {
        if isKeySelected(root: root, mode: mode, in: keys) { return true }
        let labels = keyLabels(root: root, mode: mode)
        return facets.contains { labels.contains($0.value) && $0.count > 0 }
    }

    // MARK: Ranges

    /// The quick range matching exactly (min, max), if any.
    static func selectedRange(in ranges: [QuickRange], min: Double?, max: Double?) -> QuickRange? {
        ranges.first { $0.min == min && $0.max == max }
    }

    /// Single-select: choosing the selected range clears it, otherwise sets it.
    static func toggleRange(_ range: QuickRange, min: inout Double?, max: inout Double?) {
        if min == range.min, max == range.max {
            min = nil
            max = nil
        } else {
            min = range.min
            max = range.max
        }
    }

    // MARK: Active chips of the bar

    /// What an active chip in the filter bar stands for.
    enum ActiveKind: Equatable, Sendable {
        case category(String), key(root: String), tempo, duration, tag(String)
    }

    /// One removable chip of the filter bar.
    struct ActiveChip: Identifiable, Equatable, Sendable {
        let kind: ActiveKind
        let title: String
        let hue: Theme.Hue
        var id: String {
            switch kind {
            case .category(let v): return "category-\(v)"
            case .key(let r): return "key-\(r)"
            case .tempo: return "tempo"
            case .duration: return "duration"
            case .tag(let v): return "tag-\(v)"
            }
        }
    }

    /// Splits a stored key label into root and minor flag (`C#m` → `C#`, true).
    private static func split(_ key: String) -> (root: String, minor: Bool) {
        key.hasSuffix("m") ? (String(key.dropLast()), true) : (key, false)
    }

    /// One chip per selected value, in section order: categories (by
    /// family), keys (one per root: `A`, `Am`, or `A any` when both), tempo,
    /// duration, tags.
    static func activeChips(_ f: FilterState) -> [ActiveChip] {
        var chips: [ActiveChip] = []
        let cats = f.categories.sorted {
            (CategoryFamily.of($0).rawValue, $0) < (CategoryFamily.of($1).rawValue, $1)
        }
        for c in cats {
            chips.append(ActiveChip(kind: .category(c), title: Format.categoryTitle(c), hue: Theme.categoryHue(c) ?? .fx))
        }
        let byRoot = Dictionary(grouping: f.keys.map(split), by: \.root)
        let orderedRoots = byRoot.keys.sorted { (roots.firstIndex(of: $0) ?? roots.count, $0) < (roots.firstIndex(of: $1) ?? roots.count, $1) }
        for root in orderedRoots {
            let minors = byRoot[root]?.map(\.minor) ?? []
            let title = minors.contains(true) && minors.contains(false) ? "\(root) any" : (minors.contains(true) ? root + "m" : root)
            chips.append(ActiveChip(kind: .key(root: root), title: title, hue: .property))
        }
        if let s = FilterState.rangeSummary(min: f.bpmMin, max: f.bpmMax) {
            chips.append(ActiveChip(kind: .tempo, title: "\(s) BPM", hue: .property))
        }
        if let s = FilterState.rangeSummary(min: f.durationMinSec, max: f.durationMaxSec, unit: "s") {
            chips.append(ActiveChip(kind: .duration, title: s, hue: .property))
        }
        for t in f.tags.sorted() { chips.append(ActiveChip(kind: .tag(t), title: t, hue: .tag)) }
        return chips
    }

    /// Removes what `chip` stands for (the chip's own `×`). Removing the last
    /// key also drops "Include compatible keys", which needs a key.
    static func clear(_ chip: ActiveChip, in f: inout FilterState) {
        switch chip.kind {
        case .category(let v): f.categories.remove(v)
        case .key(let root):
            f.keys = f.keys.filter { split($0).root != root }
            if f.keys.isEmpty { f.compatibleKeys = false }
        case .tempo: f.bpmMin = nil; f.bpmMax = nil
        case .duration: f.durationMinSec = nil; f.durationMaxSec = nil
        case .tag(let v): f.tags.remove(v)
        }
    }
}
