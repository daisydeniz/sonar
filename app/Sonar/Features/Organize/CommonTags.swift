import Foundation

/// Tags that every list contains, in the order of `lists[0]` (display only:
/// the inspector's chips for a multi-selection).
func commonTags(_ lists: [[String]]) -> [String] {
    guard let first = lists.first else { return [] }
    let others = lists.dropFirst().map(Set.init)
    return first.filter { tag in others.allSatisfy { $0.contains(tag) } }
}
