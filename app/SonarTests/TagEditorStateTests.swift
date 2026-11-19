import XCTest
@testable import Sonar

final class TagEditorStateTests: XCTestCase {
    private let library = [TagInfo(name: "Dark", files: 5), TagInfo(name: "club", files: 3), TagInfo(name: "analog", files: 1)]

    private func state(common: [String] = []) -> TagEditorState {
        TagEditorState(library: library, common: common)
    }

    func testToggleAddsAndRemoves() {
        var s = state()
        s.toggle("club")
        XCTAssertEqual(s.chips, ["club"])
        s.toggle("CLUB")
        XCTAssertEqual(s.chips, [])
    }

    func testCreateRowFirstWhenNoExactMatch() {
        var s = state()
        s.text = "dar"
        XCTAssertEqual(s.rows.first, .create("dar"))
        XCTAssertEqual(s.rows.count, 2)
        s.text = "dark"
        XCTAssertEqual(s.rows, [.existing(name: "Dark", files: 5, chosen: false)])
    }

    func testReturnCreatesAndClearsText() {
        var s = state()
        s.text = "punchy"
        s.activateHighlighted()
        XCTAssertEqual(s.chips, ["punchy"])
        XCTAssertEqual(s.text, "")
        s.text = "PUNCHY"  // a chip created in this sheet is a known tag now
        XCTAssertEqual(s.rows, [.existing(name: "punchy", files: 0, chosen: true)])
    }

    func testDedupeReusesExistingSpelling() {
        var s = state()
        s.add("dark")
        s.add("DARK")
        XCTAssertEqual(s.chips, ["Dark"])
        XCTAssertEqual(s.added, ["Dark"])
    }

    func testCommaPasteSplits() {
        var s = state()
        s.text = "club, Dark ,new,"
        s.textChanged()
        XCTAssertEqual(s.chips, ["club", "Dark", "new"])
        XCTAssertEqual(s.text, "")
        s.text = "a, b"
        s.textChanged()
        XCTAssertEqual(s.chips, ["club", "Dark", "new", "a"])
        XCTAssertEqual(s.text, "b")
        s.commitText()
        XCTAssertEqual(s.chips.last, "b")
    }

    func testHighlightNavigationAndToggle() {
        var s = state()
        s.text = "a"
        let rows = s.rows
        XCTAssertEqual(s.highlight, 0)
        s.moveHighlight(by: -1)
        XCTAssertEqual(s.highlight, 0)
        s.moveHighlight(by: 5)
        XCTAssertEqual(s.highlight, rows.count - 1)
        s.moveHighlight(by: -1)
        s.activateHighlighted()
        XCTAssertEqual(s.chips.count, 1)
    }

    func testRemoveLastAndNoChanges() {
        var s = state(common: ["a", "b"])
        XCTAssertFalse(s.hasChanges)
        s.removeLast()
        XCTAssertEqual(s.chips, ["a"])
        XCTAssertEqual(s.removed, ["b"])
        XCTAssertTrue(s.hasChanges)
        s.add("B")
        XCTAssertFalse(s.hasChanges)
    }

    func testMultiSelectionDiffOnlyTouchesCommonTags() {
        // Files [x, y] and [x, z] share only "x"; "y" / "z" are never chips.
        let common = commonTags([["x", "y"], ["x", "z"]])
        var s = state(common: common)
        XCTAssertEqual(s.chips, ["x"])
        s.remove("x")
        s.add("club")
        XCTAssertEqual(s.removed, ["x"])
        XCTAssertEqual(s.added, ["club"])
    }

    func testExactMatchRanksFirst() {
        var s = TagEditorState(
            library: [TagInfo(name: "808 kick", files: 4), TagInfo(name: "kick", files: 2), TagInfo(name: "kicks", files: 1)],
            common: [])
        s.text = "Kick"
        XCTAssertEqual(s.rows.first, .existing(name: "kick", files: 2, chosen: false))
        XCTAssertEqual(s.rows.dropFirst().first, .existing(name: "kicks", files: 1, chosen: false))
        s.activateHighlighted()
        XCTAssertEqual(s.chips, ["kick"])
    }
}
