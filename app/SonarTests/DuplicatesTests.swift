import XCTest
@testable import Sonar

private struct FixturesMissing: Error {}

/// Duplicates view (T6.9b) through `AppModel` on `sonar fixtures` libraries:
/// the assertions look at what the core grouped and what hide did.
@MainActor
final class DuplicatesTests: XCTestCase {
    private func fixturesDir() throws -> String {
        let env = ProcessInfo.processInfo.environment
        guard let dir = env["SONAR_FIXTURES_DIR"], !dir.isEmpty else {
            if env["SONAR_REQUIRE_FIXTURES"] == "1" { throw FixturesMissing() }
            throw XCTSkip("SONAR_FIXTURES_DIR not set (scripts/check.sh sets it)")
        }
        return dir
    }

    private func tempDir(_ name: String) -> URL {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("sonar-\(name)-\(UUID().uuidString)")
        addTeardownBlock { try? FileManager.default.removeItem(at: url) }
        return url
    }

    private func startedModel() async throws -> AppModel {
        let home = tempDir("dupes-home")
        let path = home.path
        let engine = try await Task.detached { try SonarEngine.open(home: path) }.value
        let model = AppModel(
            engine: engine, preview: AudioPreview(player: FakePlayer()), activePollMs: 20, idlePollMs: 50)
        addTeardownBlock { @MainActor in model.stop() }
        await model.start()
        model.stop()
        return model
    }

    /// Waits for analysis (near duplicates need it).
    private func waitForIndexing(_ model: AppModel) async throws {
        let deadline = Date().addingTimeInterval(180)
        while true {
            model.pollOnce()
            await model.settle()
            await model.refreshResults()
            if !model.progress.running, model.facets?.categories.isEmpty == false { return }
            if Date() >= deadline { return XCTFail("indexing timed out") }
            try await Task.sleep(nanoseconds: 50_000_000)
        }
    }

    /// A library holding three byte copies of one fixture (exact group).
    private func copiesLibrary() throws -> (dir: URL, files: [URL]) {
        let source = URL(fileURLWithPath: try fixturesDir()).appendingPathComponent("oneshots")
        let wav = try XCTUnwrap(
            FileManager.default.enumerator(at: source, includingPropertiesForKeys: nil)?
                .compactMap { $0 as? URL }.filter { $0.pathExtension == "wav" }.sorted { $0.path < $1.path }.first)
        let dir = tempDir("dupes-lib")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let copies = ["a.wav", "b.wav", "c.wav"].map { dir.appendingPathComponent($0) }
        for copy in copies { try FileManager.default.copyItem(at: wav, to: copy) }
        return (dir, copies)
    }

    func testExactGroupListsPathSizeFormatAndKeepOneHidesTheOthers() async throws {
        let model = try await startedModel()
        let lib = try copiesLibrary()
        await model.addLibrary(path: lib.dir.path)
        XCTAssertNil(model.errorMessage)

        await model.runDuplicates()
        XCTAssertEqual(model.duplicates.phase, .loaded)
        XCTAssertEqual(model.duplicates.exactGroups.count, 1)
        let group = try XCTUnwrap(model.duplicates.exactGroups.first)
        XCTAssertEqual(group.members.count, 3)
        // `row.name` is the display name without extension; members are sorted by path.
        XCTAssertEqual(group.members.map(\.row.name), ["a", "b", "c"])
        XCTAssertTrue(group.members.allSatisfy { $0.format == "WAV" && $0.size > 0 })
        // The core stores canonical paths (/private/var/... for temp dirs).
        XCTAssertEqual(
            Set(group.members.map(\.row.path)), Set(lib.files.map { URL(fileURLWithPath: String(cString: realpath($0.path, nil))).path }))
        XCTAssertEqual(group.wastedBytes, group.members[0].size * 2)

        // Selecting a member makes it the focused (previewed) row.
        let keeper = try XCTUnwrap(group.members.first { $0.row.name == "b" })
        model.selectDuplicate(keeper, in: group)
        XCTAssertEqual(model.selectedFileId, keeper.row.fileId)

        await model.keepOnly(keeper, in: group)
        XCTAssertNil(model.errorMessage)
        let after = try XCTUnwrap(model.duplicates.exactGroups.first)
        for m in after.members { XCTAssertEqual(m.row.hidden, m.row.fileId != keeper.row.fileId, m.row.name) }
        // Nothing is deleted from disk.
        XCTAssertTrue(lib.files.allSatisfy { FileManager.default.fileExists(atPath: $0.path) })

        // Unhide brings one back.
        let hidden = try XCTUnwrap(after.members.first { $0.row.hidden })
        await model.setDuplicateHidden(hidden, on: false)
        XCTAssertEqual(model.duplicates.exactGroups.first?.members.filter(\.row.hidden).count, 1)
    }

    func testRevealUsesTheFilePath() async throws {
        let model = try await startedModel()
        let lib = try copiesLibrary()
        await model.addLibrary(path: lib.dir.path)
        await model.runDuplicates()
        let member = try XCTUnwrap(model.duplicates.groups.first?.members.first)
        var revealed: [URL] = []
        model.keyboard.reveal = { revealed = $0 }
        model.revealDuplicate(member)
        XCTAssertEqual(revealed, [URL(fileURLWithPath: member.row.path)])
    }

    func testFixtureLibraryHasLabelledNearGroups() async throws {
        let model = try await startedModel()
        await model.addLibrary(path: try fixturesDir())
        try await waitForIndexing(model)
        await model.runDuplicates()
        XCTAssertEqual(model.duplicates.phase, .loaded)
        XCTAssertNil(model.errorMessage)
        XCTAssertFalse(model.duplicates.nearGroups.isEmpty, "fixture A4 variants form near groups")
        XCTAssertTrue(model.duplicates.nearGroups.allSatisfy { $0.kind == .near && $0.members.count >= 2 })
        XCTAssertTrue(model.duplicates.exactGroups.allSatisfy { $0.kind == .exact })
        XCTAssertEqual(model.duplicates.groups.count, model.duplicates.exactGroups.count + model.duplicates.nearGroups.count)
    }

    func testCancelDropsTheResultWithoutError() async throws {
        let model = try await startedModel()
        await model.addLibrary(path: try fixturesDir())
        let run = Task { await model.runDuplicates() }
        await Task.yield()
        model.cancelDuplicates()
        await run.value
        XCTAssertNil(model.errorMessage)
        if case .failed = model.duplicates.phase { XCTFail("cancel must not fail the view") }
        XCTAssertNotEqual(model.duplicates.phase, .loading)
    }

    func testSidebarEntryDoesNotQueryResults() async throws {
        let model = try await startedModel()
        model.sidebarSelection = .duplicates
        XCTAssertFalse(model.showsFilterBar)
        await model.refreshResults()
        XCTAssertNil(model.errorMessage)
    }
}
