import XCTest
@testable import Sonar

private struct FixturesMissing: Error {}

/// Organize actions (T6.8) through `AppModel` on the `sonar fixtures`
/// library: the assertions look at what the core returned.
@MainActor
final class OrganizeTests: XCTestCase {
    private func rows(_ model: AppModel) -> [FileRow] {
        (0..<model.resultCount).compactMap { model.pager.peek($0) }
    }

    private func indexedModel() async throws -> AppModel {
        let env = ProcessInfo.processInfo.environment
        guard let fixtures = env["SONAR_FIXTURES_DIR"], !fixtures.isEmpty else {
            if env["SONAR_REQUIRE_FIXTURES"] == "1" { throw FixturesMissing() }
            throw XCTSkip("SONAR_FIXTURES_DIR not set (scripts/check.sh sets it)")
        }
        let home = FileManager.default.temporaryDirectory
            .appendingPathComponent("sonar-organize-test-\(UUID().uuidString)")
        addTeardownBlock { try? FileManager.default.removeItem(at: home) }
        let path = home.path
        let engine = try await Task.detached { try SonarEngine.open(home: path) }.value
        let model = AppModel(engine: engine, activePollMs: 20, idlePollMs: 50)
        addTeardownBlock { @MainActor in model.stop() }
        await model.start()
        model.stop()
        await model.addLibrary(path: fixtures)
        let deadline = Date().addingTimeInterval(180)
        while true {
            model.pollOnce()
            await model.settle()
            await model.refreshResults()
            if !model.progress.running, model.facets?.categories.isEmpty == false { break }
            if Date() >= deadline { XCTFail("indexing timed out"); break }
            try await Task.sleep(nanoseconds: 50_000_000)
        }
        XCTAssertNil(model.errorMessage)
        return model
    }

    /// Selects the first `n` loaded rows (model-level, no audio).
    @discardableResult
    private func select(_ model: AppModel, _ n: Int) -> [Int64] {
        let picked = Array(rows(model).prefix(n))
        model.selectedFileIDs = picked.map(\.fileId)
        if let first = picked.first { model.updateSelectedSnapshot(first) }
        return picked.map(\.fileId)
    }

    // MARK: Favorite / hide

    func testFavoriteTwoSelectedShowsInFavoritesView() async throws {
        let model = try await indexedModel()
        let ids = select(model, 2)
        await model.toggleFavoriteOfSelection()
        XCTAssertNil(model.errorMessage)
        model.sidebarSelection = .favorites
        await model.refreshResults()
        XCTAssertEqual(Set(rows(model).map(\.fileId)), Set(ids))
        // All favorite -> the toggle turns them off.
        select(model, 2)
        await model.toggleFavoriteOfSelection()
        XCTAssertEqual(model.resultCount, 0)
    }

    func testHideRemovesFromSearchAndShowHiddenBringsBack() async throws {
        let model = try await indexedModel()
        let total = model.resultCount
        let ids = select(model, 2)
        await model.setHidden(fileIds: ids, on: true)
        XCTAssertEqual(model.resultCount, total - 2)
        XCTAssertTrue(Set(rows(model).map(\.fileId)).isDisjoint(with: ids))
        XCTAssertNil(model.selectedRow, "hidden focused row leaves the inspector")

        model.organizeState.showHidden = true
        await model.refreshResults()
        XCTAssertEqual(model.resultCount, total)
        let hidden = rows(model).filter { ids.contains($0.fileId) }
        XCTAssertEqual(hidden.count, 2)
        XCTAssertTrue(hidden.allSatisfy(\.hidden))
        XCTAssertEqual(model.rowMenuEntries(for: hidden).first { $0.title.hasSuffix("hide") }?.title, "Unhide")

        await model.setHidden(fileIds: ids, on: false)
        model.organizeState.showHidden = false
        await model.refreshResults()
        XCTAssertEqual(model.resultCount, total)
        XCTAssertTrue(rows(model).allSatisfy { !$0.hidden })
    }

    // MARK: Tags

    func testAddAndRemoveTagOnTwoFiles() async throws {
        let model = try await indexedModel()
        let ids = select(model, 2)
        await model.addTags("snare, punchy ,,snare")
        let tagged = rows(model).filter { ids.contains($0.fileId) }
        XCTAssertEqual(tagged.count, 2)
        XCTAssertTrue(tagged.allSatisfy { Set($0.tags) == ["snare", "punchy"] })
        XCTAssertEqual(Set(model.organizeState.tags.map(\.name)), ["snare", "punchy"])
        XCTAssertEqual(model.selectedRow?.tags.sorted(), ["punchy", "snare"], "inspector row refreshed")

        await model.removeTag("snare")
        XCTAssertTrue(rows(model).filter { ids.contains($0.fileId) }.allSatisfy { $0.tags == ["punchy"] })
        XCTAssertEqual(AppModel.parseTags(" a,b , ,a"), ["a", "b"])
    }

    // MARK: Collections

    func testCollectionLifecycle() async throws {
        let model = try await indexedModel()
        let ids = select(model, 3)
        let created = await model.createCollection(name: " Drums ", adding: ids)
        let id = try XCTUnwrap(created)
        XCTAssertNil(model.errorMessage)
        XCTAssertEqual(model.collections.map(\.name), ["Drums"])
        XCTAssertEqual(model.collections.first?.files, 3)

        model.sidebarSelection = .collection(id)
        await model.refreshResults()
        XCTAssertEqual(Set(rows(model).map(\.fileId)), Set(ids))
        let entries = model.rowMenuEntries(for: Array(rows(model).prefix(1)))
        XCTAssertTrue(entries.contains { $0.title == "Remove from Collection" })

        await model.renameCollection(id: id, name: "Kicks")
        XCTAssertEqual(model.collections.map(\.name), ["Kicks"])

        await model.removeFromCollection(id: id, fileIds: [ids[0]])
        XCTAssertEqual(Set(rows(model).map(\.fileId)), Set(ids.dropFirst()))
        XCTAssertEqual(model.collections.first?.files, 2)

        await model.deleteCollection(id: id)
        XCTAssertTrue(model.collections.isEmpty)
        XCTAssertEqual(model.sidebarSelection, .allSamples)
        XCTAssertGreaterThan(model.resultCount, 3, "files stay in the library")
    }

    func testPromptCreatesCollection() async throws {
        let model = try await indexedModel()
        let ids = select(model, 1)
        model.organizeState.ask(.newCollection(ids))
        model.organizeState.promptText = "Loops"
        await model.commitPrompt()
        XCTAssertNil(model.organizeState.prompt)
        XCTAssertEqual(model.collections.map(\.name), ["Loops"])
        XCTAssertEqual(model.collections.first?.files, 1)
    }

    // MARK: Overrides

    func testBpmOverrideFiltersAndRevertRestoresDetected() async throws {
        let model = try await indexedModel()
        let ids = select(model, 2)
        let detected = rows(model).filter { ids.contains($0.fileId) }.map(\.bpm)

        let ok = await model.setOverride(.bpm, value: "128")
        XCTAssertTrue(ok)
        model.filters.bpmMin = 127
        model.filters.bpmMax = 129
        await model.refreshResults()
        XCTAssertTrue(Set(ids).isSubset(of: Set(rows(model).map(\.fileId))))
        XCTAssertTrue(rows(model).filter { ids.contains($0.fileId) }.allSatisfy { $0.bpm == 128 })

        await model.revertOverride(.bpm)
        let after = rows(model).filter { ids.contains($0.fileId) }
        XCTAssertTrue(after.isEmpty || after.allSatisfy { $0.bpm != 128 || detected.contains(128) })
        model.clearFilters()
        await model.refreshResults()
        XCTAssertEqual(rows(model).filter { ids.contains($0.fileId) }.map(\.bpm), detected)
    }

    func testInvalidBpmShowsInlineErrorWithoutCrash() async throws {
        let model = try await indexedModel()
        select(model, 1)
        let before = model.selectedRow?.bpm
        let ok = await model.setOverride(.bpm, value: "abc")
        XCTAssertFalse(ok)
        XCTAssertNotNil(model.organizeState.overrideError)
        XCTAssertNil(model.errorMessage, "inline, not the modal alert")
        XCTAssertEqual(model.selectedRow?.bpm, before)
        let okAgain = await model.setOverride(.category, value: model.organizeState.categories[0])
        XCTAssertTrue(okAgain)
        XCTAssertNil(model.organizeState.overrideError, "a good edit clears the message")
    }

    func testCategoryPopupListsCoreCategories() throws {
        let state = OrganizeState()
        XCTAssertEqual(state.categories, categoryIds())
        XCTAssertFalse(state.categories.isEmpty)
    }
}
