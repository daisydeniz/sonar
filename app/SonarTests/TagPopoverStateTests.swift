import XCTest
@testable import Sonar

/// Tag popover state and the "Tags ›" menu (T12.7, DESIGN §4.12): pure
/// Swift, no core.
@MainActor
final class TagPopoverStateTests: XCTestCase {
    private let library = [("warm", 58), ("dark", 50), ("club", 50), ("groove", 44), ("lofi", 12), ("low end", 7), ("loose", 3)]
        .map { TagInfo(name: $0.0, files: UInt64($0.1)) }

    private func state(_ files: [[String]]) -> TagPopoverState {
        TagPopoverState(
            library: library, files: files.enumerated().map { (id: Int64($0.offset + 1), tags: $0.element) })
    }

    private func names(_ rows: [TagPopoverState.Row]) -> [String] {
        rows.map { row in
            switch row {
            case .tag(let name, _, _): return name
            case .create(let name): return "+\(name)"
            }
        }
    }

    func testToggleAddsThenRemoves() {
        var s = state([[], []])
        let add = s.toggle("dark")
        XCTAssertEqual(add, TagChange(kind: .add, tag: "dark", fileIds: [1, 2]))
        XCTAssertEqual(s.mark("dark"), .all)
        let remove = s.toggle("DARK")
        XCTAssertEqual(remove, TagChange(kind: .remove, tag: "dark", fileIds: [1, 2]))
        XCTAssertEqual(s.mark("dark"), .none)
        XCTAssertTrue(s.chips.isEmpty)
    }

    func testMixedBecomesAllOnToggleAndOnlyLackingFilesChange() {
        var s = state([["groove"], ["groove"], []])
        XCTAssertEqual(s.mark("groove"), .some(2))
        XCTAssertEqual(s.mixedCaption(s.mark("groove")), "2/3")
        let change = s.toggle("groove")
        XCTAssertEqual(change, TagChange(kind: .add, tag: "groove", fileIds: [3]))
        XCTAssertEqual(s.mark("groove"), .all)
    }

    func testCreateUsesExistingSpellingAndNormalises() {
        var s = state([[]])
        s.text = "  Dark "
        s.moveHighlight(by: 0)
        XCTAssertEqual(s.activateHighlighted(), [TagChange(kind: .add, tag: "dark", fileIds: [1])])
        XCTAssertEqual(s.text, "")
        var n = state([[]])
        n.text = "  night   drive "
        XCTAssertEqual(names(n.rows).last, "+night drive")
        n.moveHighlight(by: 1)
        XCTAssertEqual(n.activateHighlighted().first?.tag, "night drive")
    }

    func testMatchOrderPrefixThenContainsCreateLast() {
        var s = state([[]])
        s.text = "lo"
        XCTAssertEqual(names(s.rows), ["lofi", "low end", "loose", "+lo"])
        XCTAssertEqual(s.highlight, 0)
        s.text = "e"
        XCTAssertEqual(names(s.rows), ["groove", "low end", "loose", "+e"])
        s.text = "lofi"
        XCTAssertFalse(names(s.rows).contains { $0.hasPrefix("+") }, "exact match hides Create")
    }

    func testEmptyFieldShowsMostUsedCappedAtEight() {
        let many = (1...12).map { TagInfo(name: "t\($0)", files: UInt64(100 - $0)) }
        let s = TagPopoverState(library: many, files: [(id: 1, tags: ["t2"])])
        XCTAssertEqual(s.rows.count, 8)
        XCTAssertEqual(names(s.rows).first, "t1")
        XCTAssertEqual(s.mostUsed[1], .tag(name: "t2", files: 98, mark: .all))
    }

    func testCommaCommitsPiecesAndKeepsRest() {
        var s = state([[]])
        s.text = "a, b,c"
        let changes = s.textChanged()
        XCTAssertEqual(changes.map(\.tag), ["a", "b"])
        XCTAssertEqual(s.text, "c")
        XCTAssertEqual(s.commitText().map(\.tag), ["c"])
    }

    func testRemoveLastChipAndHighlightBounds() {
        var s = state([["warm", "dark"]])
        XCTAssertEqual(s.removeLast()?.tag, "dark")
        XCTAssertEqual(s.chips.map(\.name), ["warm"])
        s.moveHighlight(by: -1)
        XCTAssertEqual(s.highlight, s.rows.count - 1)
        s.moveHighlight(by: 100)
        XCTAssertEqual(s.highlight, s.rows.count - 1)
    }

    func testUndoGroupingInverseReversesAll() {
        let steps = [
            TagChange(kind: .add, tag: "a", fileIds: [1, 2]), TagChange(kind: .remove, tag: "b", fileIds: [2]),
        ]
        XCTAssertEqual(
            TagChange.inverse(of: steps),
            [TagChange(kind: .add, tag: "b", fileIds: [2]), TagChange(kind: .remove, tag: "a", fileIds: [1, 2])])
    }

    // MARK: Menu

    func testMenuTriStateForSelection() async throws {
        let model = AppModel(settings: AppSettings(defaults: UserDefaults(suiteName: "tags-menu-\(UUID().uuidString)")!))
        var rows = SnapshotFixtures.rows(count: 3)
        rows[0].tags = ["warm", "groove"]
        rows[1].tags = ["warm"]
        rows[2].tags = ["warm"]
        model.installStaticState(libraries: SnapshotFixtures.libraries, collections: [])
        _ = try await model.pager.reset(loader: SnapshotPageLoader(rows: rows))
        model.organizeState.tags = library
        let ids = rows.map(\.fileId)
        let tags = model.tagMenuEntry(for: ids)
        XCTAssertEqual(tags.title, "Tags")
        let marks = Dictionary(uniqueKeysWithValues: tags.children.filter { !$0.isSeparator }.map { ($0.title, $0.mark) })
        XCTAssertEqual(marks["warm"], .on)
        XCTAssertEqual(marks["groove"], .mixed)
        XCTAssertEqual(marks["dark"], .off)
        let tail = tags.children.suffix(3).map(\.title)
        XCTAssertEqual(tail, ["", "Add Tag…", "Manage Tags…"])
        XCTAssertEqual(tags.children.first { $0.title == "Add Tag…" }?.shortcut, "t")
        tags.children.last?.action?()
        XCTAssertTrue(model.organizeState.showsManageTags, "Manage Tags… opens the sheet")
    }
}
