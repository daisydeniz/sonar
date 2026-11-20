import Foundation

/// Pure rules of the Manage Tags sheet (DESIGN §4.12 layer 3): ordering,
/// filtering, the similar-pair wording and which spelling survives a merge.
/// Matching and normalisation themselves are the core's.
enum TagManageLogic {
    /// Most used first, ties and unused tags alphabetical.
    static func sorted(_ tags: [TagInfo]) -> [TagInfo] {
        tags.sorted {
            if $0.files != $1.files { return $0.files > $1.files }
            return $0.name.localizedCaseInsensitiveCompare($1.name) == .orderedAscending
        }
    }

    /// Tags whose name contains `filter` (case-insensitive); all when empty.
    static func filtered(_ tags: [TagInfo], by filter: String) -> [TagInfo] {
        let needle = filter.trimmingCharacters(in: .whitespaces)
        guard !needle.isEmpty else { return tags }
        return tags.filter { $0.name.localizedCaseInsensitiveContains(needle) }
    }

    /// "1 letter", "hyphen" or "spelling" next to a similar pair.
    static func reasonText(_ reason: SimilarTagReason) -> String {
        switch reason {
        case .typo: return "1 letter"
        case .separators: return "hyphen"
        case .prefix: return "spelling"
        }
    }

    /// The spelling that survives merging `pair` (the more used one; the
    /// first on a tie) and the one merged into it.
    static func survivor(of pair: SimilarTagPair) -> (keep: String, drop: String) {
        pair.bFiles > pair.aFiles ? (pair.b, pair.a) : (pair.a, pair.b)
    }

    /// "Merge All": one `(sources, target)` per surviving name. Pairs that
    /// chain (a~b, b~c) resolve through the names already merged away, so no
    /// call names a tag that is gone.
    static func mergePlan(_ pairs: [SimilarTagPair]) -> [(sources: [String], target: String)] {
        var alias: [String: String] = [:]
        func resolve(_ name: String) -> String {
            var current = name.lowercased()
            while let next = alias[current] { current = next }
            return current
        }
        var spelling: [String: String] = [:]
        var order: [String] = []
        var merged: [String: [String]] = [:]
        for pair in pairs {
            let (keep, drop) = survivor(of: pair)
            for name in [keep, drop] where spelling[name.lowercased()] == nil {
                spelling[name.lowercased()] = name
            }
            let k = resolve(keep), d = resolve(drop)
            guard k != d else { continue }
            alias[d] = k
            let inherited = merged.removeValue(forKey: d) ?? []
            merged[k, default: []].append(contentsOf: [drop] + inherited)
            if !order.contains(k) { order.append(k) }
            order.removeAll { $0 == d }
        }
        return order.compactMap { key in
            guard let target = spelling[key], let sources = merged[key] else { return nil }
            return (sources, target)
        }
    }

    /// Core-style normalisation of a typed name: trimmed, inner whitespace
    /// runs collapsed (case is the core's business).
    static func collapsed(_ name: String) -> String {
        name.split(whereSeparator: \.isWhitespace).joined(separator: " ")
    }

    /// The sidebar's tags: used ones only, most used first.
    static func sidebarTags(_ tags: [TagInfo]) -> [TagInfo] { sorted(tags.filter { $0.files > 0 }) }

    /// How many tags the sidebar lists before "Show All".
    static let sidebarLimit = 5

    /// Sidebar rows: the 5 most used tags (all when `expanded`), plus the tag
    /// the results are scoped to when it would otherwise be hidden.
    static func sidebarVisible(_ tags: [TagInfo], expanded: Bool, selected: String?) -> [TagInfo] {
        let used = sidebarTags(tags)
        guard !expanded, used.count > sidebarLimit else { return used }
        var shown = Array(used.prefix(sidebarLimit))
        if let selected, !shown.contains(where: { $0.name == selected }),
            let extra = used.first(where: { $0.name == selected })
        {
            shown.append(extra)
        }
        return shown
    }
}
