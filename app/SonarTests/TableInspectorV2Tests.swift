import AppKit
import XCTest
@testable import Sonar

/// T12.6: playing-row symbol, toolbar scope text, inspector formats, sizes.
@MainActor
final class TableInspectorV2Tests: XCTestCase {
    private let libs: [(id: Int64, name: String)] = [(1, "KSHMR Vol 1"), (2, "Drum Hits")]

    func testScopeTitle() {
        let cols: [(id: Int64, name: String)] = [(7, "Techno Kit")]
        XCTAssertEqual(ScopeLabel.title(.allSamples, libraries: libs, collections: cols), "All Samples")
        XCTAssertEqual(ScopeLabel.title(.favorites, libraries: libs, collections: cols), "Favorites")
        XCTAssertEqual(ScopeLabel.title(.library(2), libraries: libs, collections: cols), "Drum Hits")
        XCTAssertEqual(ScopeLabel.title(.collection(7), libraries: libs, collections: cols), "Techno Kit")
    }

    func testScopeSubtitleListsLibrariesInScope() {
        XCTAssertEqual(ScopeLabel.subtitle(.allSamples, libraries: libs), "KSHMR Vol 1 \u{00B7} Drum Hits")
        XCTAssertEqual(ScopeLabel.subtitle(.library(1), libraries: libs), "")
        XCTAssertEqual(ScopeLabel.subtitle(.allSamples, libraries: []), "")
        let many: [(id: Int64, name: String)] = (1...5).map { (Int64($0), "L\($0)") }
        XCTAssertEqual(
            ScopeLabel.subtitle(.favorites, libraries: many), "L1 \u{00B7} L2 \u{00B7} L3 + 2 more")
    }

    func testInspectorFormats() {
        XCTAssertEqual(Format.sampleRate(44_100), "44,100 Hz")
        XCTAssertNil(Format.sampleRate(nil))
        XCTAssertEqual(Format.channels(1), "Mono")
        XCTAssertEqual(Format.channels(2), "Stereo")
        XCTAssertEqual(Format.channels(6), "6")
    }

    func testTableSizesAndPlayingRowSymbol() async throws {
        XCTAssertEqual(Theme.Size.rowHeight, 26)
        XCTAssertEqual(Theme.Size.headerHeight, 24)
        let pager = ResultsPager(pageSize: 20)
        let controller = ResultsTableController(pager: pager)
        let rows = SnapshotFixtures.rows()
        _ = try await pager.reset(loader: StalledPageLoader(rows: rows))
        XCTAssertEqual(controller.tableView.rowHeight, 26)
        XCTAssertEqual(controller.tableView.headerView?.frame.height, 24)
        let tv = controller.tableView
        let column = try XCTUnwrap(tv.tableColumns.first)
        controller.playingFileID = rows[0].fileId
        let playing = try XCTUnwrap(controller.tableView(tv, viewFor: column, row: 0) as? FavoriteCellView)
        XCTAssertTrue(playing.isPlaying)
        XCTAssertTrue(playing.isHeartVisible, "the speaker shows without hover")
        let other = try XCTUnwrap(controller.tableView(tv, viewFor: column, row: 1) as? FavoriteCellView)
        XCTAssertFalse(other.isPlaying)
    }
}
