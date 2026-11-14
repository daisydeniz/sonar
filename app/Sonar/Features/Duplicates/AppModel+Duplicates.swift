import AppKit
import Foundation

/// Duplicates view actions (T6.9b). Grouping is the core's; "keep one" is
/// only hide/unhide through the organize setters. Nothing here deletes files.
extension AppModel {
    /// Runs the duplicate finder (both kinds, calibrated threshold) off the
    /// main actor. Latest wins: a rerun or `cancelDuplicates()` supersedes it.
    func runDuplicates() async {
        guard let bridge else { return }
        duplicates.generation += 1
        let mine = duplicates.generation
        duplicates.setPhase(.loading)
        do {
            let result = try await bridge.duplicates(DuplicatesQuery(exact: true, near: true, threshold: nil))
            guard mine == duplicates.generation else { return }
            let rows = try await bridge.rows(fileIds: Array(Set(result.groups.flatMap(\.fileIds))))
            guard mine == duplicates.generation else { return }
            duplicates.apply(result, rows: rows)
        } catch FfiError.Cancelled {
            if mine == duplicates.generation { duplicates.setPhase(.idle) }
        } catch {
            guard mine == duplicates.generation else { return }
            duplicates.setPhase(.failed(Self.describe(error)))
        }
    }

    /// Stops a running search (the view went away).
    func cancelDuplicates() {
        duplicates.generation += 1
        bridge?.cancelDuplicates()
        if duplicates.phase == .loading { duplicates.setPhase(.idle) }
    }

    /// Selecting a member previews it (auto-play), shows it in the inspector
    /// and pre-decodes the rest of its group.
    func selectDuplicate(_ member: DuplicateMember?, in group: DuplicateGroupItem? = nil) {
        duplicates.selection = member?.id
        selectedFileIDs = member.map { [$0.row.fileId] } ?? []
        // A single preview start (a second select would restart the clip).
        select(member?.row, neighbours: group?.members.map(\.row).filter { $0.fileId != member?.row.fileId } ?? [])
    }

    /// Reveals the file in Finder.
    func revealDuplicate(_ member: DuplicateMember) {
        keyboard.reveal([URL(fileURLWithPath: member.row.path)])
    }

    /// Keeps `keeper` visible and hides the other members of its group.
    func keepOnly(_ keeper: DuplicateMember, in group: DuplicateGroupItem) async {
        let others = group.members.filter { $0.row.fileId != keeper.row.fileId && !$0.row.hidden }.map(\.row.fileId)
        if keeper.row.hidden { await setHidden(fileIds: [keeper.row.fileId], on: false) }
        if !others.isEmpty { await setHidden(fileIds: others, on: true) }
        deselectDuplicate(ifHidden: others)
        await refreshDuplicateRows()
    }

    /// Hides or unhides one member.
    func setDuplicateHidden(_ member: DuplicateMember, on: Bool) async {
        await setHidden(fileIds: [member.row.fileId], on: on)
        if on { deselectDuplicate(ifHidden: [member.row.fileId]) }
        await refreshDuplicateRows()
    }

    /// Clears the list selection (and preview) when the selected file was just
    /// hidden; `setHidden` only clears selections found in the results pager.
    private func deselectDuplicate(ifHidden fileIds: [Int64]) {
        guard let selected = duplicates.member(duplicates.selection), fileIds.contains(selected.row.fileId) else { return }
        selectDuplicate(nil)
    }

    /// Re-reads the rows shown (hidden flags) without re-running the search.
    func refreshDuplicateRows() async {
        guard let bridge else { return }
        do {
            duplicates.update(rows: try await bridge.rows(fileIds: duplicates.allFileIDs))
        } catch {
            errorMessage = Self.describe(error)
        }
    }
}
