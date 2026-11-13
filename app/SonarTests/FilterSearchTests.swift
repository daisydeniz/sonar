import AppKit
import XCTest
@testable import Sonar

private struct FixturesMissing: Error {}

/// Filter chips + facets through `AppModel` on the `sonar fixtures` library
/// (core-side filtering: the assertions look at the rows the core returned),
/// plus pure tests of `FilterState`, the facet query rule and key routing.
@MainActor
final class FilterSearchTests: XCTestCase {
    private func makeHome() -> URL {
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("sonar-filter-test-\(UUID().uuidString)")
        addTeardownBlock { try? FileManager.default.removeItem(at: url) }
        return url
    }

    private func rows(_ model: AppModel) -> [FileRow] {
        (0..<model.resultCount).compactMap { model.pager.peek($0) }
    }

    /// A model over the indexed fixtures library.
    private func indexedModel() async throws -> AppModel {
        let env = ProcessInfo.processInfo.environment
        guard let fixtures = env["SONAR_FIXTURES_DIR"], !fixtures.isEmpty else {
            if env["SONAR_REQUIRE_FIXTURES"] == "1" { throw FixturesMissing() }
            throw XCTSkip("SONAR_FIXTURES_DIR not set (scripts/check.sh sets it)")
        }
        let home = makeHome().path
        let engine = try await Task.detached { try SonarEngine.open(home: home) }.value
        let model = AppModel(engine: engine, activePollMs: 20, idlePollMs: 50)
        addTeardownBlock { @MainActor in model.stop() }
        await model.start()
        model.stop()  // no background poll: this test drives pollOnce itself
        await model.addLibrary(path: fixtures)
        // Indexing may not have started yet right after addLibrary, so wait
        // for "idle AND categories analysed", not just "not running".
        let deadline = Date().addingTimeInterval(180)
        while true {
            model.pollOnce()
            await model.settle()
            await model.refreshResults()
            if !model.progress.running, model.facets?.categories.isEmpty == false { break }
            if Date() >= deadline {
                XCTFail("indexing timed out: running=\(model.progress.running) done=\(model.progress.done)/\(model.progress.total) total=\(model.resultCount) facets=\(String(describing: model.facets))")
                break
            }
            try await Task.sleep(nanoseconds: 50_000_000)
        }
        XCTAssertNil(model.errorMessage)
        return model
    }

    // MARK: Facets and chips through the core

    func testFacetsPopulateAndCategoryChipNarrows() async throws {
        let model = try await indexedModel()
        let total = model.resultCount
        let facets = try XCTUnwrap(model.facets, "facets are requested on page 0")
        XCTAssertGreaterThan(facets.categories.count, 1, "total=\(total) facets=\(facets)")
        // Files with no known category (corrupt fixtures) are not counted.
        XCTAssertLessThanOrEqual(facets.categories.reduce(0) { $0 + Int($1.count) }, total)
        XCTAssertEqual(facets.libraries.reduce(0) { $0 + Int($1.count) }, total)

        let chosen = try XCTUnwrap(facets.categories.first)
        model.filters.categories = [chosen.value]
        await model.refreshResults()
        XCTAssertEqual(model.resultCount, Int(chosen.count))
        XCTAssertTrue(rows(model).allSatisfy { $0.category == chosen.value })
        // drill-sideways: the other categories are still listed with counts
        let after = try XCTUnwrap(model.facets)
        XCTAssertEqual(Set(after.categories.map(\.value)), Set(facets.categories.map(\.value)))
        let chips = FacetChip.chips(facets: after.categories, selected: model.filters.categories)
        XCTAssertEqual(chips.filter(\.selected).map(\.value), [chosen.value])

        model.clearFilters()
        await model.refreshResults()
        XCTAssertEqual(model.resultCount, total, "clearing restores the total")
    }

    func testKindKeyBpmFavoriteFiltersAreAppliedByTheCore() async throws {
        let model = try await indexedModel()
        let total = model.resultCount
        let facets = try XCTUnwrap(model.facets)

        // kind
        let kind = try XCTUnwrap(facets.kinds.first)
        model.filters.kinds = [kind.value]
        await model.refreshResults()
        XCTAssertEqual(model.resultCount, Int(kind.count))
        XCTAssertTrue(rows(model).allSatisfy { $0.kind == kind.value })

        // key, then compatible keys can only widen the set
        model.clearFilters()
        let key = try XCTUnwrap(facets.keys.first)
        model.filters.keys = [key.value]
        await model.refreshResults()
        XCTAssertEqual(model.resultCount, Int(key.count))
        XCTAssertTrue(rows(model).allSatisfy { $0.key == key.value })
        model.filters.compatibleKeys = true
        await model.refreshResults()
        XCTAssertGreaterThanOrEqual(model.resultCount, Int(key.count))
        XCTAssertTrue(rows(model).contains { $0.key == key.value })

        // BPM range
        model.clearFilters()
        model.filters.bpmMin = 90
        model.filters.bpmMax = 130
        await model.refreshResults()
        XCTAssertLessThan(model.resultCount, total)
        let bpms = rows(model).map(\.bpm)
        XCTAssertTrue(bpms.allSatisfy { ($0 ?? 0) >= 90 && ($0 ?? 0) <= 130 })

        // favourites
        model.clearFilters()
        let pick = try XCTUnwrap(rows(model).first)
        await model.setFavorite(fileIds: [pick.fileId], on: true)
        model.filters.favoritesOnly = true
        await model.refreshResults()
        XCTAssertEqual(rows(model).map(\.fileId), [pick.fileId])
        XCTAssertEqual(model.facets?.favorites, 1)

        model.clearFilters()
        XCTAssertFalse(model.filters.isActive)
        await model.refreshResults()
        XCTAssertEqual(model.resultCount, total)
    }

    func testSidebarLibraryWinsOverLibraryChips() async throws {
        let model = try await indexedModel()
        let lib = try XCTUnwrap(model.libraries.first)
        model.sidebarSelection = .library(lib.id)
        model.filters.libraryIds = [lib.id + 999]
        XCTAssertFalse(model.showsLibraryChips)
        await model.refreshResults()
        XCTAssertGreaterThan(model.resultCount, 0, "chip libraries are ignored under a sidebar library")
    }

    // MARK: Pure logic

    func testFilterStateApplyMergesSidebarAndChips() {
        var base = SearchFilters()
        base.favoritesOnly = true
        base.libraryIds = [7]
        var state = FilterState()
        state.categories = ["Kick", "Snare"]
        state.keys = ["Am"]
        state.compatibleKeys = true
        state.libraryIds = [1, 2]
        state.durationMinSec = 0.5
        let f = state.apply(to: base)
        XCTAssertEqual(f.categories, ["Kick", "Snare"])
        XCTAssertTrue(f.compatibleKeys)
        XCTAssertEqual(f.libraryIds, [7], "sidebar library wins")
        XCTAssertTrue(f.favoritesOnly)
        XCTAssertEqual(f.durationMinMs, 500)

        state.keys = []
        XCTAssertFalse(state.apply(to: SearchFilters()).compatibleKeys, "no key selected: no compatible keys")
        XCTAssertTrue(state.isActive)
        XCTAssertFalse(FilterState().isActive)
    }

    func testDurationAndBpmBoundsNeverTrap() {
        var state = FilterState()
        state.durationMaxSec = 1e20
        state.durationMinSec = -5
        state.bpmMax = .infinity
        var f = state.apply(to: SearchFilters())
        XCTAssertEqual(f.durationMaxMs, UInt64(1e12 * 1000))
        XCTAssertEqual(f.durationMinMs, 0)
        XCTAssertNil(f.bpmMax)
        state.durationMaxSec = .infinity
        state.durationMinSec = .nan
        f = state.apply(to: SearchFilters())
        XCTAssertNil(f.durationMaxMs)
        XCTAssertNil(f.durationMinMs)
    }

    func testSelectedChipsStayVisibleAtZeroCount() {
        let chips = FacetChip.chips(
            facets: [FacetCount(value: "Kick", count: 3)], selected: ["Kick", "Pad"])
        XCTAssertEqual(chips.map(\.value), ["Kick", "Pad"])
        XCTAssertEqual(chips.last?.count, 0)
        XCTAssertTrue(chips.allSatisfy(\.selected))
    }

    func testFacetsRequestedOnlyForFirstPage() async throws {
        let home = makeHome().path
        let engine = try await Task.detached { try SonarEngine.open(home: home) }.value
        let loader = EngineResultLoader(
            bridge: CoreBridge(engine: engine), query: SearchQuery(text: "x"), wantsFacets: true)
        XCTAssertTrue(loader.pageQuery(offset: 0, limit: 200).facets)
        XCTAssertFalse(loader.pageQuery(offset: 200, limit: 200).facets)
        let plain = EngineResultLoader(bridge: CoreBridge(engine: engine), query: SearchQuery(text: "x"))
        XCTAssertFalse(plain.pageQuery(offset: 0, limit: 200).facets)
    }

    // MARK: Keyboard

    private func keyEvent(_ code: UInt16, modifiers: NSEvent.ModifierFlags = []) throws -> NSEvent {
        try XCTUnwrap(NSEvent.keyEvent(
            with: .keyDown, location: .zero, modifierFlags: modifiers, timestamp: 0, windowNumber: 0,
            context: nil, characters: " ", charactersIgnoringModifiers: " ", isARepeat: false,
            keyCode: code))
    }

    func testTableKeyMapping() throws {
        XCTAssertEqual(TableKey.from(try keyEvent(49)), .space)
        XCTAssertEqual(TableKey.from(try keyEvent(36)), .returnKey)
        XCTAssertEqual(TableKey.from(try keyEvent(53)), .escape)
        XCTAssertNil(TableKey.from(try keyEvent(125)), "arrows stay native")
        XCTAssertNil(TableKey.from(try keyEvent(49, modifiers: .command)))
    }

    func testKeyRoutingSpaceReturnEscape() throws {
        let model = AppModel()
        var toggled = 0
        var revealed: [URL] = []
        var focusedSearch = 0
        model.keyboard.onTogglePreview = { toggled += 1 }
        model.keyboard.reveal = { revealed += $0 }
        model.keyboard.focusSearch = { focusedSearch += 1 }
        let row = FileRow(
            fileId: 1, libraryId: 1, path: "/tmp/a.wav", name: "a.wav", folders: "", category: nil,
            kind: nil, bpm: nil, key: nil, durationMs: nil, sampleRate: nil, channels: nil,
            favorite: false, hidden: false, tags: [])
        model.select(row)

        // through the real NSTableView subclass
        let table = KeyTableView()
        table.onKey = { model.keyboard.handle($0, selected: model.selectedRow) }
        table.keyDown(with: try keyEvent(49))
        table.keyDown(with: try keyEvent(36))
        table.keyDown(with: try keyEvent(53))
        XCTAssertEqual(toggled, 1)
        XCTAssertEqual(model.keyboard.previewToggleRequests, 1)
        XCTAssertEqual(revealed, [URL(fileURLWithPath: "/tmp/a.wav")])
        XCTAssertEqual(focusedSearch, 1)

        model.select(nil)
        model.revealSelected()
        XCTAssertEqual(revealed.count, 1, "nothing selected: nothing revealed")
    }
}
