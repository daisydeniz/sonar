import XCTest
@testable import Sonar

private struct FixturesMissing: Error {}

/// Drives `AppModel` (not the views) on the `sonar fixtures` library:
/// open -> add library -> indexing (pause/resume) -> search -> favourites
/// sidebar -> remove library. Same fixtures env convention as
/// `EngineBridgeTests`.
@MainActor
final class AppModelTests: XCTestCase {
    /// Fresh temp home per call; removed when the test ends.
    private func makeHome() -> URL {
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("sonar-model-test-\(UUID().uuidString)")
        addTeardownBlock { try? FileManager.default.removeItem(at: url) }
        return url
    }

    private func fixturesDir() throws -> String {
        let env = ProcessInfo.processInfo.environment
        guard let dir = env["SONAR_FIXTURES_DIR"], !dir.isEmpty else {
            if env["SONAR_REQUIRE_FIXTURES"] == "1" { throw FixturesMissing() }
            throw XCTSkip("SONAR_FIXTURES_DIR not set (scripts/check.sh sets it)")
        }
        return dir
    }

    private func waitUntil(timeout: TimeInterval = 180, _ what: String, _ cond: () -> Bool) async throws {
        let deadline = Date().addingTimeInterval(timeout)
        while !cond() {
            if Date() >= deadline { return XCTFail("timed out waiting for \(what)") }
            try await Task.sleep(nanoseconds: 20_000_000)
        }
    }

    func testShellFlow() async throws {
        let fixtures = try fixturesDir()
        let homePath = makeHome().path
        let engine = try await Task.detached { try SonarEngine.open(home: homePath) }.value
        let model = AppModel(engine: engine, activePollMs: 20, idlePollMs: 50)
        defer { model.stop() }

        // open
        await model.start()
        XCTAssertEqual(model.engineState, .ready)
        XCTAssertTrue(model.libraries.isEmpty)
        XCTAssertTrue(model.rows.isEmpty)

        // add library: listed, scanned, indexing started
        await model.addLibrary(path: fixtures)
        XCTAssertNil(model.errorMessage)
        XCTAssertEqual(model.libraries.count, 1)
        XCTAssertFalse(model.rows.isEmpty, "files are searchable right after the scan")

        // pause then resume while indexing (a fast job may already be done)
        model.pauseIndexing()
        if model.progress.running { XCTAssertTrue(model.progress.paused) }
        model.resumeIndexing()
        XCTAssertFalse(model.progress.paused)

        // progress reaches finished
        try await waitUntil("indexing to finish") {
            model.pollOnce()
            return !model.progress.running
        }

        // search text updates results
        model.searchText = "hit"
        await model.refreshResults()
        XCTAssertGreaterThanOrEqual(model.rows.count, 12)
        XCTAssertLessThan(model.rows.count, Int(AppModel.pageLimit))
        let hit = try XCTUnwrap(model.rows.first)
        model.selectedFileId = hit.fileId
        XCTAssertEqual(model.selectedRow?.fileId, hit.fileId)

        // favourites sidebar shows only the favourited row
        model.searchText = ""
        await model.setFavorite(fileIds: [hit.fileId], on: true)
        model.sidebarSelection = .favorites
        await model.refreshResults()
        XCTAssertEqual(model.rows.map(\.fileId), [hit.fileId])
        XCTAssertEqual(model.rows.first?.favorite, true)

        // a collection selection shows its rows
        let cid = try engine.createCollection(name: "Picks")
        _ = try engine.addToCollection(id: cid, fileIds: [hit.fileId])
        await model.reloadSidebar()
        XCTAssertEqual(model.collections.map(\.name), ["Picks"])
        model.sidebarSelection = .collection(cid)
        await model.refreshResults()
        XCTAssertEqual(model.rows.map(\.fileId), [hit.fileId])

        // latest wins: overlapping refreshes leave the newest state
        model.sidebarSelection = .allSamples
        model.searchText = "hit"
        let first = Task { await model.refreshResults() }
        await Task.yield()  // let `first` capture "hit" and suspend in search
        model.searchText = "zzzznomatch"
        await model.refreshResults()
        await first.value
        XCTAssertTrue(model.rows.isEmpty, "the older 'hit' result must not win")
        XCTAssertNil(model.errorMessage, "a superseded search must not surface an error")

        // remove library empties everything
        let libraryId = try XCTUnwrap(model.libraries.first?.id)
        model.searchText = ""
        model.sidebarSelection = .library(libraryId)
        await model.removeLibrary(id: libraryId)
        XCTAssertTrue(model.libraries.isEmpty)
        XCTAssertEqual(model.sidebarSelection, .allSamples)
        XCTAssertTrue(model.rows.isEmpty)
    }

    func testOpenFailureIsAnErrorNotACrash() async throws {
        // A file where the home directory should be makes open() fail.
        let home = makeHome()
        try Data().write(to: home)
        let path = home.path
        let engine = try? await Task.detached { try SonarEngine.open(home: path) }.value
        XCTAssertNil(engine)
    }
}
