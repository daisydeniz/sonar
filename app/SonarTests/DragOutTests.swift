import AppKit
import XCTest
@testable import Sonar

@MainActor
final class DragOutTests: XCTestCase {
    private var dir: URL!

    override func setUp() async throws {
        dir = FileManager.default.temporaryDirectory.appendingPathComponent("sonar-dragout-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        // Files for rows 0..<6; rows 6+ are "offline" (missing on disk).
        for i in 0..<6 { FileManager.default.createFile(atPath: dir.appendingPathComponent("row\(i).wav").path, contents: Data()) }
    }

    override func tearDown() async throws {
        DragOutState.isDraggingFromTable = false
        try? FileManager.default.removeItem(at: dir)
    }

    private func makeController(rows: Int = 10, pageSize: Int = 100) async throws
        -> (ResultsTableController, ResultsPager)
    {
        let pager = ResultsPager(pageSize: pageSize)
        let controller = ResultsTableController(pager: pager)
        try await pager.reset(loader: SyntheticPageLoader(rowCount: rows, directory: dir.path))
        return (controller, pager)
    }

    func testWritersAreFileURLsForLoadedExistingRows() async throws {
        let (c, _) = try await makeController()
        let writer = c.tableView(c.tableView, pasteboardWriterForRow: 2) as? NSURL
        XCTAssertEqual(writer?.path, dir.appendingPathComponent("row2.wav").path)
        XCTAssertTrue(writer?.isFileURL ?? false)
        XCTAssertNil(c.tableView(c.tableView, pasteboardWriterForRow: 8), "missing file: no writer")
    }

    func testPlaceholderRowHasNoWriter() async throws {
        let (c, _) = try await makeController(rows: 1_000, pageSize: 100)
        XCTAssertNil(c.tableView(c.tableView, pasteboardWriterForRow: 700), "page not loaded")
    }

    func testDragMaskIsCopyOnly() {
        for mask in [ResultsTableController.externalDragMask, ResultsTableController.localDragMask] {
            XCTAssertTrue(mask.contains(.copy))
            XCTAssertFalse(mask.contains(.move))
            XCTAssertFalse(mask.contains(.delete))
        }
    }

    func testMultiSelectionAndFocusFollowLastChange() async throws {
        let (c, _) = try await makeController()
        var focused: [Int64?] = []
        var sets: [[Int64]] = []
        c.onSelect = { focused.append($0?.fileId) }
        c.onSelectionChange = { sets.append($0) }
        c.tableView.selectRowIndexes(IndexSet([1]), byExtendingSelection: false)
        c.tableView.selectRowIndexes(IndexSet([4]), byExtendingSelection: true)
        c.tableView.selectRowIndexes(IndexSet([2]), byExtendingSelection: true)
        XCTAssertEqual(c.selectedIDs, [1, 2, 4])
        XCTAssertEqual(sets.last, [1, 2, 4])
        XCTAssertEqual(focused, [1, 4, 2], "focus = most recently selected row")
        c.tableView.selectAll(nil)
        XCTAssertEqual(c.selectedIDs.count, 10)
        XCTAssertEqual(c.selectedID, 2, "select-all keeps the focus")
        XCTAssertEqual(c.tableView.selectedRow, 2)
        XCTAssertEqual(focused, [1, 4, 2], "select-all does not fire onSelect")
    }

    func testSelectionSurvivesPagerReset() async throws {
        let (c, pager) = try await makeController()
        c.tableView.selectRowIndexes(IndexSet([2, 8]), byExtendingSelection: false)
        c.tableView.selectRowIndexes(IndexSet([3]), byExtendingSelection: true)
        XCTAssertEqual(c.selectedIDs, [2, 3, 8])
        // New result list keeps ids 0..<5 only: 8 is dropped.
        try await pager.reset(loader: SyntheticPageLoader(rowCount: 5, directory: dir.path))
        XCTAssertEqual(c.selectedIDs, [2, 3])
        XCTAssertEqual(c.tableView.selectedRowIndexes, IndexSet([2, 3]))
        XCTAssertEqual(c.selectedID, 3, "focus is kept")
        XCTAssertEqual(c.tableView.selectedRow, 3)
    }

    func testSelfDropDoesNotStartSimilarity() async throws {
        let (c, _) = try await makeController()
        let url = dir.appendingPathComponent("row1.wav")
        XCTAssertTrue(SimilarityDrop.accepts([url]), "external audio file drop is accepted")
        c.dragWillBegin()
        XCTAssertFalse(SimilarityDrop.accepts([url]), "drag from our own table is ignored")
        c.dragEnded()
        XCTAssertTrue(SimilarityDrop.accepts([url]))
    }

    func testRevealUsesAllSelectedRows() async throws {
        let keyboard = KeyboardActions()
        var revealed: [URL] = []
        keyboard.reveal = { revealed = $0 }
        let rows = (0..<3).map { i in
            FileRow(
                fileId: Int64(i), libraryId: 1, path: "/x/\(i).wav", name: "\(i)", folders: "", category: nil,
                kind: nil, bpm: nil, key: nil, durationMs: 1, sampleRate: 44100, channels: 2, favorite: false,
                hidden: false, tags: [])
        }
        keyboard.revealSelected(rows[0], selection: rows)
        XCTAssertEqual(revealed.map(\.path), ["/x/0.wav", "/x/1.wav", "/x/2.wav"])
        keyboard.revealSelected(rows[1], selection: [rows[1]])
        XCTAssertEqual(revealed.map(\.path), ["/x/1.wav"])
    }
}
