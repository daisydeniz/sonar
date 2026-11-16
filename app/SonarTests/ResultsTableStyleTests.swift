import AppKit
import XCTest
@testable import Sonar

/// Display mapping and row styling of the results table (DESIGN §4.1).
@MainActor
final class ResultsTableStyleTests: XCTestCase {
    func testKindAndCategoryDisplayText() {
        XCTAssertEqual(Format.kindTitle("loop"), "Loop")
        XCTAssertEqual(Format.kindTitle("one_shot"), "One-shot")
        XCTAssertEqual(Format.kindTitle(nil), "")
        XCTAssertEqual(Format.kindSymbol("loop"), "repeat")
        XCTAssertNotNil(Format.kindSymbol("one_shot"))
        XCTAssertNil(Format.kindSymbol(nil))
        XCTAssertEqual(Format.categoryTitle("kick"), "Kick")
        XCTAssertEqual(Format.categoryTitle("fx"), "FX")
        XCTAssertEqual(Format.categoryTitle(nil), "")
        XCTAssertEqual(Format.categoryTitle(""), "")
    }

    func testMissingValuesAreEmptyInTheTableButDashInTheInspector() {
        XCTAssertEqual(Format.cellBPM(nil), "")
        XCTAssertEqual(Format.cellBPM(128), "128")
        XCTAssertEqual(Format.cellDuration(ms: nil), "")
        XCTAssertEqual(Format.cellDuration(ms: 1500), "0:01.500")
        XCTAssertEqual(Format.cellText(nil), "")
        XCTAssertEqual(Format.bpm(nil), "-")
        XCTAssertEqual(Format.text(nil), "-")
    }

    func testSelectedRowKeepsNormalInteriorStyleAndPicksTintByFocus() {
        let row = ResultsRowView(frame: NSRect(x: 0, y: 0, width: 400, height: 24))
        row.isSelected = true
        ResultsRowView.emphasisOverride = true
        defer { ResultsRowView.emphasisOverride = nil }
        XCTAssertEqual(row.interiorBackgroundStyle, .normal)
        XCTAssertTrue(row.isEmphasized)
        ResultsRowView.emphasisOverride = false
        XCTAssertEqual(row.interiorBackgroundStyle, .normal)
        XCTAssertFalse(row.isEmphasized)
    }

    func testSkeletonRowsAndMissingValuesInCells() async throws {
        let pager = ResultsPager(pageSize: 20)
        let controller = ResultsTableController(pager: pager)
        _ = try await pager.reset(loader: StalledPageLoader(rows: SnapshotFixtures.rows()))
        let tv = controller.tableView
        let name = try XCTUnwrap(tv.tableColumns.first { $0.identifier.rawValue == "name" })
        let bpm = try XCTUnwrap(tv.tableColumns.first { $0.identifier.rawValue == "bpm" })
        let loading = controller.tableView(tv, viewFor: name, row: 150) as? NameCellView
        XCTAssertEqual(loading?.label.stringValue, "")
        XCTAssertEqual(loading?.label.accessibilityLabel(), "Loading")
        let loaded = controller.tableView(tv, viewFor: name, row: 0) as? NameCellView
        XCTAssertEqual(loaded?.label.stringValue, pager.peek(0)?.name)
        // A one-shot without BPM shows nothing.
        let index = try XCTUnwrap((0..<20).first { pager.peek($0)?.bpm == nil })
        let cell = controller.tableView(tv, viewFor: bpm, row: index) as? ValueCellView
        XCTAssertEqual(cell?.label.stringValue, "")
        XCTAssertEqual(cell?.label.alignment, .right)
    }

    /// Clicking Name then BPM leaves `[bpm, name]`: only BPM shows an arrow.
    func testOnlyFirstSortDescriptorShowsIndicator() {
        let descriptors = [
            NSSortDescriptor(key: "bpm", ascending: false), NSSortDescriptor(key: "name", ascending: true),
        ]
        XCTAssertEqual(ResultsHeaderCell.sortAscending(for: "bpm", in: descriptors), false)
        XCTAssertNil(ResultsHeaderCell.sortAscending(for: "name", in: descriptors))
        XCTAssertNil(ResultsHeaderCell.sortAscending(for: "bpm", in: []))
    }

    func testHeartOutlineShowsOnHoverOnly() async throws {
        let pager = ResultsPager(pageSize: 20)
        let controller = ResultsTableController(pager: pager)
        _ = try await pager.reset(loader: StalledPageLoader(rows: SnapshotFixtures.rows()))
        let tv = controller.tableView
        let column = try XCTUnwrap(tv.tableColumns.first)
        // Row 0 is not a favourite (fixture: every 17th id).
        let cell = try XCTUnwrap(controller.tableView(tv, viewFor: column, row: 0) as? FavoriteCellView)
        XCTAssertFalse(cell.isFavorite)
        XCTAssertFalse(cell.isHeartVisible)
        cell.setRowHovered(true)
        XCTAssertTrue(cell.isHeartVisible)
        cell.setRowHovered(false)
        XCTAssertFalse(cell.isHeartVisible)
    }
}
