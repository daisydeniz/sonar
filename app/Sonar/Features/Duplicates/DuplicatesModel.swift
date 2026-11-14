import Foundation
import Observation

/// One file of a duplicate group, with the size the core reported.
struct DuplicateMember: Identifiable, Equatable, Sendable {
    /// Unique per group (a file may be in one exact and one near group).
    let id: String
    let row: FileRow
    let size: UInt64

    /// Upper-case file extension ("WAV"), display only.
    var format: String {
        let ext = URL(fileURLWithPath: row.path).pathExtension
        return ext.isEmpty ? "-" : ext.uppercased()
    }
}

/// A group as the view shows it (order and membership come from the core).
struct DuplicateGroupItem: Identifiable, Equatable, Sendable {
    let id: Int
    let kind: DuplicateKind
    var members: [DuplicateMember]
    let wastedBytes: UInt64
    let maxDistance: Float
}

/// Load state of the Duplicates view.
enum DuplicatesPhase: Equatable, Sendable {
    case idle
    case loading
    case loaded
    case failed(String)
}

/// View state of the Duplicates view (T6.9b). The grouping itself is the
/// core's (`duplicates()`); this only holds what was returned. The work is in
/// `AppModel+Duplicates`.
@MainActor
@Observable
final class DuplicatesModel {
    private(set) var phase: DuplicatesPhase = .idle
    private(set) var groups: [DuplicateGroupItem] = []
    /// Files looked at / compared for near duplicates in the last run.
    private(set) var files: UInt64 = 0
    private(set) var nearFiles: UInt64 = 0
    private(set) var skipped: UInt64 = 0
    /// Selected member (`DuplicateMember.id`).
    var selection: String?
    @ObservationIgnored var generation = 0

    var exactGroups: [DuplicateGroupItem] { groups.filter { $0.kind == .exact } }
    var nearGroups: [DuplicateGroupItem] { groups.filter { $0.kind == .near } }

    func setPhase(_ phase: DuplicatesPhase) { self.phase = phase }

    /// Replaces the groups with a core result; `rows` resolves the ids.
    func apply(_ result: DuplicatesResult, rows: [FileRow]) {
        let byId = Dictionary(rows.map { ($0.fileId, $0) }, uniquingKeysWith: { a, _ in a })
        groups = result.groups.enumerated().compactMap { index, g in
            let members = zip(g.fileIds, g.sizes).compactMap { id, size in
                byId[id].map { DuplicateMember(id: "\(index)-\(id)", row: $0, size: size) }
            }
            guard members.count >= 2 else { return nil }
            // Stable display order inside a group: by path (core order is by id).
            let sorted = members.sorted { $0.row.path.localizedStandardCompare($1.row.path) == .orderedAscending }
            return DuplicateGroupItem(
                id: index, kind: g.kind, members: sorted, wastedBytes: g.wastedBytes, maxDistance: g.maxDistance)
        }
        files = result.files
        nearFiles = result.nearFiles
        skipped = result.skipped
        if let key = selection, member(key) == nil { selection = nil }
        phase = .loaded
    }

    /// Swaps in fresher rows (after hide/unhide); groups are unchanged.
    func update(rows: [FileRow]) {
        let byId = Dictionary(rows.map { ($0.fileId, $0) }, uniquingKeysWith: { a, _ in a })
        for g in groups.indices {
            for m in groups[g].members.indices {
                let old = groups[g].members[m]
                if let fresh = byId[old.row.fileId] {
                    groups[g].members[m] = DuplicateMember(id: old.id, row: fresh, size: old.size)
                }
            }
        }
    }

    /// The member with this view id.
    func member(_ id: String?) -> DuplicateMember? {
        guard let id else { return nil }
        for g in groups { if let m = g.members.first(where: { $0.id == id }) { return m } }
        return nil
    }

    /// The group holding the member with this view id.
    func group(of id: String?) -> DuplicateGroupItem? {
        guard let id else { return nil }
        return groups.first { $0.members.contains { $0.id == id } }
    }

    /// Every file id shown (for refreshing rows).
    var allFileIDs: [Int64] {
        Array(Set(groups.flatMap { $0.members.map(\.row.fileId) }))
    }
}
