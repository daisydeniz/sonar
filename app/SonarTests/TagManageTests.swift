import XCTest
@testable import Sonar

private struct FixturesMissing: Error {}

private func pair(
    _ a: String, _ aFiles: UInt64, _ b: String, _ bFiles: UInt64, _ reason: SimilarTagReason = .typo
) -> SimilarTagPair {
    SimilarTagPair(a: a, aFiles: aFiles, b: b, bFiles: bFiles, reason: reason)
}

/// Pure rules of the Manage Tags sheet and the sidebar Tags section.
@MainActor
final class TagManageLogicTests: XCTestCase {
    func testSortedFilteredAndReasonText() {
        let tags = [TagInfo(name: "b", files: 2), TagInfo(name: "Amen", files: 0), TagInfo(name: "a", files: 2),
            TagInfo(name: "warm", files: 9)]
        XCTAssertEqual(TagManageLogic.sorted(tags).map(\.name), ["warm", "a", "b", "Amen"])
        XCTAssertEqual(TagManageLogic.filtered(tags, by: " AM ").map(\.name), ["Amen"])
        XCTAssertEqual(TagManageLogic.filtered(tags, by: "").count, 4)
        XCTAssertEqual(TagManageLogic.reasonText(.typo), "1 letter")
        XCTAssertEqual(TagManageLogic.reasonText(.separators), "hyphen")
        XCTAssertEqual(TagManageLogic.reasonText(.prefix), "spelling")
        XCTAssertEqual(TagManageLogic.collapsed("  lo   fi "), "lo fi")
    }

    func testMoreUsedSpellingSurvives() {
        XCTAssertEqual(TagManageLogic.survivor(of: pair("grove", 2, "groove", 44)).keep, "groove")
        XCTAssertEqual(TagManageLogic.survivor(of: pair("groove", 44, "grove", 2)).drop, "grove")
        XCTAssertEqual(TagManageLogic.survivor(of: pair("a", 3, "b", 3)).keep, "a", "tie keeps the first")
    }

    func testMergeAllResolvesChains() {
        let plan = TagManageLogic.mergePlan([
            pair("groove", 44, "grove", 2), pair("lofi", 12, "lo-fi", 5), pair("grove", 2, "groov", 1),
        ])
        XCTAssertEqual(plan.count, 2)
        let groove = plan.first { $0.target == "groove" }
        XCTAssertEqual(Set(groove?.sources ?? []), ["grove", "groov"], "the chain ends in one target")
        XCTAssertEqual(plan.first { $0.target == "lofi" }?.sources, ["lo-fi"])
        XCTAssertTrue(TagManageLogic.mergePlan([]).isEmpty)
    }

    func testSidebarShowsFiveMostUsedAndTheScopedTag() {
        let tags = (1...8).map { TagInfo(name: "t\($0)", files: UInt64(100 - $0)) } + [TagInfo(name: "unused", files: 0)]
        XCTAssertEqual(TagManageLogic.sidebarVisible(tags, expanded: false, selected: nil).map(\.name),
            ["t1", "t2", "t3", "t4", "t5"])
        XCTAssertEqual(TagManageLogic.sidebarVisible(tags, expanded: false, selected: "t7").map(\.name).last, "t7")
        XCTAssertEqual(TagManageLogic.sidebarVisible(tags, expanded: true, selected: nil).count, 8, "unused tags stay out")
        XCTAssertEqual(TagManageLogic.sidebarVisible(Array(tags.prefix(3)), expanded: false, selected: nil).count, 3)
    }

    func testTagScopeAddsTheTagToTheQuery() {
        var base = SearchFilters()
        base.tags = ["dark"]
        XCTAssertEqual(AppModel.scoped(base, to: .tag("warm")).tags, ["dark", "warm"])
        XCTAssertEqual(AppModel.scoped(base, to: .tag("DARK")).tags, ["dark"], "already filtered, any case")
        XCTAssertEqual(AppModel.scoped(base, to: .allSamples).tags, ["dark"])
        XCTAssertEqual(ScopeLabel.title(.tag("warm"), libraries: [], collections: []), "warm")
    }
}

/// Sheet model state without the core.
@MainActor
final class TagManageModelTests: XCTestCase {
    func testStateOfTheSheet() {
        let tags = [TagInfo(name: "warm", files: 58), TagInfo(name: "grove", files: 1), TagInfo(name: "amen", files: 0)]
        let model = TagManageModel(app: AppModel(), tags: tags, pairs: [pair("groove", 4, "grove", 1)])
        XCTAssertEqual(model.phase, .ready)
        XCTAssertEqual(model.subtitle, "3 tags · All libraries")
        XCTAssertEqual(model.hint, "Double-click a tag to rename it")
        XCTAssertFalse(model.canMerge)
        model.selection = ["warm", "grove"]
        XCTAssertTrue(model.canMerge)
        XCTAssertEqual(model.hint, "2 selected · double-click to rename")
        XCTAssertEqual(model.deleteTitle, "Delete 2 tags?")
        XCTAssertTrue(model.deleteMessage.contains("59"), model.deleteMessage)
        model.selection = ["grove"]
        XCTAssertEqual(model.deleteMessage, "“grove” will be removed from 1 sample. The samples stay.")
        model.selection = ["amen"]
        XCTAssertEqual(model.deleteMessage, "“amen” is not used by any sample.")
        model.filter = "wa"
        XCTAssertEqual(model.visibleTags.map(\.name), ["warm"])
        XCTAssertFalse(model.canCreate, "no samples selected in the results")
    }
}

/// Rename / merge / delete / ignore through the FFI on the `sonar fixtures`
/// library, each with its Undo, plus dropping rows on a sidebar tag.
@MainActor
final class TagManageCoreTests: XCTestCase {
    private func indexedModel() async throws -> AppModel {
        let env = ProcessInfo.processInfo.environment
        guard let fixtures = env["SONAR_FIXTURES_DIR"], !fixtures.isEmpty else {
            if env["SONAR_REQUIRE_FIXTURES"] == "1" { throw FixturesMissing() }
            throw XCTSkip("SONAR_FIXTURES_DIR not set (scripts/check.sh sets it)")
        }
        let home = FileManager.default.temporaryDirectory
            .appendingPathComponent("sonar-tagmanage-test-\(UUID().uuidString)")
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

    private func ids(_ model: AppModel, _ n: Int) -> [Int64] {
        (0..<model.resultCount).compactMap { model.pager.peek($0)?.fileId }.prefix(n).map { $0 }
    }

    private func tags(_ model: AppModel, _ ids: [Int64]) async throws -> [[String]] {
        let rows = try await XCTUnwrap(model.bridge).rows(fileIds: ids)
        let byId = Dictionary(rows.map { ($0.fileId, $0.tags.sorted()) }, uniquingKeysWith: { a, _ in a })
        return ids.map { byId[$0] ?? [] }
    }

    /// An undo manager that groups explicitly (no run loop in tests).
    private func manager(_ model: AppModel) -> UndoManager {
        let manager = UndoManager()
        manager.groupsByEvent = false
        model.organizeState.undoManager = manager
        return manager
    }

    private func undoable(_ manager: UndoManager, _ body: () async -> Void) async {
        manager.beginUndoGrouping()
        await body()
        manager.endUndoGrouping()
    }

    /// Three files: [groove, warm], [groove], [grove].
    private func seeded() async throws -> (AppModel, [Int64], UndoManager) {
        let model = try await indexedModel()
        let id = ids(model, 3)
        XCTAssertEqual(id.count, 3)
        await model.addTags(names: ["groove", "warm"], to: [id[0]])
        await model.addTags(names: ["groove"], to: [id[1]])
        await model.addTags(names: ["grove"], to: [id[2]])
        return (model, id, manager(model))
    }

    func testRenameThenUndoAndRedo() async throws {
        let (model, id, manager) = try await seeded()
        await undoable(manager) { await model.renameTag(from: "warm", to: "hot") }
        var now = try await tags(model, id)
        XCTAssertEqual(now[0], ["groove", "hot"])
        XCTAssertTrue(model.organizeState.tags.contains { $0.name == "hot" })
        XCTAssertFalse(model.organizeState.tags.contains { $0.name == "warm" })

        manager.undo()
        await model.settle()
        now = try await tags(model, id)
        XCTAssertEqual(now[0], ["groove", "warm"])
        manager.redo()
        await model.settle()
        now = try await tags(model, id)
        XCTAssertEqual(now[0], ["groove", "hot"])
    }

    func testRenameOntoExistingMergesAndUndoRestoresBoth() async throws {
        let (model, id, manager) = try await seeded()
        await undoable(manager) { await model.renameTag(from: "grove", to: "groove") }
        var now = try await tags(model, id)
        XCTAssertEqual(now, [["groove", "warm"], ["groove"], ["groove"]])

        manager.undo()
        await model.settle()
        now = try await tags(model, id)
        XCTAssertEqual(now, [["groove", "warm"], ["groove"], ["grove"]], "files that had groove keep it")
    }

    func testMergeManyIntoOneAndUndo() async throws {
        let (model, id, manager) = try await seeded()
        await undoable(manager) { await model.mergeTags(["grove", "warm"], into: "groove") }
        var now = try await tags(model, id)
        XCTAssertEqual(now, [["groove"], ["groove"], ["groove"]])
        XCTAssertEqual(model.organizeState.tags.map(\.name).sorted(), ["groove"])

        manager.undo()
        await model.settle()
        now = try await tags(model, id)
        XCTAssertEqual(now, [["groove", "warm"], ["groove"], ["grove"]])
    }

    func testDeleteAndUndo() async throws {
        let (model, id, manager) = try await seeded()
        model.sidebarSelection = .tag("groove")
        await undoable(manager) { await model.deleteTags(["groove"]) }
        var now = try await tags(model, id)
        XCTAssertEqual(now, [["warm"], [], ["grove"]])
        XCTAssertEqual(model.sidebarSelection, .allSamples, "a deleted tag cannot stay the scope")

        manager.undo()
        await model.settle()
        now = try await tags(model, id)
        XCTAssertEqual(now, [["groove", "warm"], ["groove"], ["grove"]])
    }

    func testIgnoredPairIsNotSuggestedAgain() async throws {
        let (model, _, _) = try await seeded()
        let snapshot = try await model.manageTagsSnapshot()
        let suggested = try XCTUnwrap(
            snapshot.pairs.first { Set([$0.a, $0.b]) == ["groove", "grove"] }, "groove / grove look alike")
        XCTAssertEqual(TagManageLogic.survivor(of: suggested).keep, "groove")
        await model.ignoreSimilarPair(suggested)
        let after = try await model.manageTagsSnapshot()
        XCTAssertNil(after.pairs.first { Set([$0.a, $0.b]) == ["groove", "grove"] })
        XCTAssertEqual(Set(after.tags.map(\.name)), ["groove", "warm", "grove"])
    }

    func testSheetModelMergeAllAndDelete() async throws {
        let (model, id, manager) = try await seeded()
        let sheet = TagManageModel(app: model)
        await sheet.reload()
        XCTAssertEqual(sheet.phase, .ready)
        XCTAssertEqual(sheet.tags.map(\.name).first, "groove", "most used first")
        XCTAssertFalse(sheet.pairs.isEmpty)

        manager.beginUndoGrouping()
        await sheet.mergeAll()
        manager.endUndoGrouping()
        XCTAssertTrue(sheet.pairs.isEmpty)
        var now = try await tags(model, id)
        XCTAssertEqual(now[2], ["groove"])

        sheet.selection = ["warm"]
        manager.beginUndoGrouping()
        await sheet.deleteSelection()
        manager.endUndoGrouping()
        XCTAssertTrue(sheet.selection.isEmpty)
        now = try await tags(model, id)
        XCTAssertEqual(now[0], ["groove"])
        XCTAssertNil(model.errorMessage)
    }

    func testDroppingRowsOnATagAddsItAsOneUndoStep() async throws {
        let (model, id, manager) = try await seeded()
        manager.beginUndoGrouping()
        let ok = await model.dropRowsOnTag("warm", ids: [id[1], id[2]])
        manager.endUndoGrouping()
        XCTAssertTrue(ok)
        var now = try await tags(model, id)
        XCTAssertEqual(now, [["groove", "warm"], ["groove", "warm"], ["grove", "warm"]])

        manager.undo()
        await model.settle()
        now = try await tags(model, id)
        XCTAssertEqual(now, [["groove", "warm"], ["groove"], ["grove"]], "one Undo removes it from both rows")
        let selected = Array(id.prefix(1))
        model.selectedFileIDs = selected
        manager.beginUndoGrouping()
        let dropped = await model.dropRowsOnTag("dark")
        manager.endUndoGrouping()
        XCTAssertTrue(dropped, "defaults to the selection")
        now = try await tags(model, selected)
        XCTAssertTrue(now[0].contains("dark"), "selection got the dropped tag")
    }

    /// The core caps a page at 1000 rows even when more are asked for; the
    /// Undo snapshot must still see every file (offset advances by rows returned).
    func testPagedFileIdsAdvancesByReturnedRows() async throws {
        let total = 3500
        let ids = try await CoreBridge.pagedFileIds { offset, limit in
            let end = min(Int(offset) + min(Int(limit), 1000), total)
            let rows = (Int(offset)..<end).map { i in
                FileRow(
                    fileId: Int64(i), libraryId: 1, path: "/p/\(i).wav", name: "\(i)", folders: "",
                    category: "", kind: "loop", bpm: nil, key: nil, durationMs: 1, sampleRate: 44100,
                    channels: 2, favorite: false, hidden: false, tags: [])
            }
            return SearchPage(rows: rows, total: UInt64(total), hasMore: end < total, mode: .text,
                              fallback: nil, facets: nil)
        }
        XCTAssertEqual(ids, (0..<total).map(Int64.init), "no file skipped past the 1000-row cap")
    }
}
