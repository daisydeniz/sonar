import XCTest
@testable import Sonar

private struct FixturesMissing: Error {}

/// Records the queries a fake similarity searcher receives.
private actor QueryLog {
    private(set) var queries: [SimilarQuery] = []
    func add(_ q: SimilarQuery) { queries.append(q) }
}

/// Holds one fake query until `release()`; `waitUntilBlocked()` returns once
/// a query is parked (deterministic: no sleeps).
private actor Gate {
    private var blocked = false
    private var released = false
    private var waiter: CheckedContinuation<Void, Never>?
    private var parked: CheckedContinuation<Void, Never>?

    func park() async {
        blocked = true
        waiter?.resume()
        waiter = nil
        if released { return }
        await withCheckedContinuation { parked = $0 }
    }

    func waitUntilBlocked() async {
        if blocked { return }
        await withCheckedContinuation { waiter = $0 }
    }

    func release() {
        released = true
        parked?.resume()
        parked = nil
    }
}

private func fakeRow(_ id: Int64, name: String? = nil) -> FileRow {
    FileRow(
        fileId: id, libraryId: 1, path: "/tmp/fake-\(id).wav", name: name ?? "fake-\(id)", folders: "",
        category: nil, kind: nil, bpm: nil, key: nil, durationMs: nil, sampleRate: nil, channels: nil,
        previewGainDb: nil, favorite: false, hidden: false, tags: [])
}

private func fakePage(_ ids: [Int64]) -> SimilarPage {
    SimilarPage(
        rows: ids.map { fakeRow($0) },
        hits: ids.enumerated().map {
            SimilarHit(fileId: $1, distance: Float($0) * 0.1, spectrum: 0, timbre: 0, pitch: 0, amplitude: 0)
        },
        candidates: UInt64(ids.count))
}

/// Similarity UI model: fake-searcher tests (deterministic latest-wins, slider
/// mapping, debounce) and fixture-driven tests through the real core.
@MainActor
final class SimilarityTests: XCTestCase {
    private func rows(_ model: AppModel) -> [FileRow] {
        (0..<model.resultCount).compactMap { model.pager.peek($0) }
    }

    private func fixturesDir() throws -> String {
        let env = ProcessInfo.processInfo.environment
        guard let dir = env["SONAR_FIXTURES_DIR"], !dir.isEmpty else {
            if env["SONAR_REQUIRE_FIXTURES"] == "1" { throw FixturesMissing() }
            throw XCTSkip("SONAR_FIXTURES_DIR not set (scripts/check.sh sets it)")
        }
        return dir
    }

    /// A model without an engine whose similarity searcher is `search`.
    private func fakeModel(_ search: @escaping @Sendable (SimilarQuery) async throws -> SimilarPage) -> AppModel {
        let model = AppModel()
        model.similarity.searchOverride = search
        model.similarity.debounceNs = 0
        return model
    }

    /// A model over the indexed fixtures library (waits for the analysis).
    private func indexedModel() async throws -> AppModel {
        let fixtures = try fixturesDir()
        let home = FileManager.default.temporaryDirectory
            .appendingPathComponent("sonar-similar-test-\(UUID().uuidString)")
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
            if Date() >= deadline {
                XCTFail("indexing timed out")
                break
            }
            try await Task.sleep(nanoseconds: 50_000_000)
        }
        XCTAssertNil(model.errorMessage)
        return model
    }

    // MARK: Fake searcher

    func testSliderChangeRequeriesWithCoreMappedWeights() async throws {
        let log = QueryLog()
        let model = fakeModel { q in
            await log.add(q)
            return fakePage([1, 2, 3])
        }
        model.findSimilar(to: fakeRow(99))
        await model.settle()
        model.setSimilarityWeight(.spectrum, 0.5)
        await model.settle()
        let queries = await log.queries
        XCTAssertEqual(queries.count, 2)
        XCTAssertEqual(queries[0].weights, try aspectWeightsFromSliders(overall: 1, spectrum: 0, timbre: 0, pitch: 0, amplitude: 0))
        XCTAssertEqual(queries[1].weights, try aspectWeightsFromSliders(overall: 1, spectrum: 0.5, timbre: 0, pitch: 0, amplitude: 0))
        XCTAssertNotEqual(queries[0].weights, queries[1].weights)
        XCTAssertEqual(queries[1].seed, .file(fileId: 99))
        model.resetSimilarityWeights()
        await model.settle()
        let all = await log.queries
        XCTAssertEqual(all.count, 3)
        XCTAssertEqual(all[2].weights, all[0].weights)
    }

    func testRapidSliderChangesCoalesceIntoOneQuery() async throws {
        let log = QueryLog()
        let model = fakeModel { q in
            await log.add(q)
            return fakePage([1])
        }
        model.findSimilar(to: fakeRow(7))
        await model.settle()
        model.similarity.debounceNs = 5_000_000
        model.setSimilarityWeight(.timbre, 0.2)
        model.setSimilarityWeight(.timbre, 0.4)
        model.setSimilarityWeight(.pitch, 0.6)
        await model.settle()
        let queries = await log.queries
        XCTAssertEqual(queries.count, 2, "initial query + one debounced re-query")
        XCTAssertEqual(queries[1].weights, try aspectWeightsFromSliders(overall: 1, spectrum: 0, timbre: 0.4, pitch: 0.6, amplitude: 0))
    }

    func testAllZeroSlidersShowCoreErrorInline() async throws {
        let model = fakeModel { _ in fakePage([1]) }
        model.findSimilar(to: fakeRow(7))
        await model.settle()
        model.setSimilarityWeight(.overall, 0)
        await model.settle()
        XCTAssertNotNil(model.similarity.message)
        XCTAssertNil(model.errorMessage)
        XCTAssertEqual(model.resultCount, 0)
    }

    func testSupersededQueryResultIsDropped() async throws {
        let gate = Gate()
        let model = fakeModel { q in
            if q.seed == .file(fileId: 1) {
                await gate.park()
                return fakePage([101, 102])
            }
            return fakePage([201, 202, 203])
        }
        model.findSimilar(to: fakeRow(1))
        await gate.waitUntilBlocked()
        model.findSimilar(to: fakeRow(2))
        while model.similarity.isLoading { await Task.yield() }
        await gate.release()
        await model.settle()
        XCTAssertEqual(rows(model).map(\.fileId), [201, 202, 203], "the older, slower query must not overwrite")
        XCTAssertEqual(model.similarity.target, .file(id: 2, name: "fake-2"))
    }

    func testCancelledQueryIsNotAnError() async throws {
        let model = fakeModel { _ in throw FfiError.Cancelled }
        model.findSimilar(to: fakeRow(1))
        await model.settle()
        XCTAssertNil(model.similarity.message)
        XCTAssertNil(model.errorMessage)
    }

    func testErrorsMapToInlineMessages() {
        let target = SimilarityTarget.external(url: URL(fileURLWithPath: "/x/y.wav"))
        XCTAssertEqual(
            SimilarityModel.message(for: FfiError.NotFound(message: "file 1"), target: nil),
            "This file isn't analyzed yet")
        XCTAssertTrue(
            SimilarityModel.message(for: FfiError.Internal(message: "no vector store yet"), target: nil)
                .contains("once indexing"))
        XCTAssertTrue(
            SimilarityModel.message(for: FfiError.Audio(message: "bad"), target: target)
                .hasPrefix("Couldn't read that audio file"))
    }

    func testTypingLeavesSimilarityMode() async throws {
        let model = fakeModel { _ in fakePage([1, 2]) }
        model.findSimilar(to: fakeRow(9))
        await model.settle()
        XCTAssertTrue(model.similarity.isActive)
        model.searchText = "kick"
        model.searchTextChanged()
        await model.settle()
        XCTAssertFalse(model.similarity.isActive)
    }

    func testDropPicksFirstAudioFile() {
        let urls = [
            URL(fileURLWithPath: "/a/readme.txt"), URL(fileURLWithPath: "/a/Kick.WAV"),
            URL(fileURLWithPath: "/a/snare.flac"),
        ]
        XCTAssertEqual(SimilarityDrop.firstAudioURL(in: urls)?.lastPathComponent, "Kick.WAV")
        XCTAssertNil(SimilarityDrop.firstAudioURL(in: [URL(fileURLWithPath: "/a/doc.pdf")]))
        XCTAssertNil(SimilarityDrop.firstAudioURL(in: [URL(string: "https://x.y/a.wav")!]))
    }

    // MARK: Real core (fixtures)

    func testFindSimilarFromLibraryFileOrdersByDistanceWithoutSeed() async throws {
        let model = try await indexedModel()
        // Corrupt/silent fixtures have no vectors: seed with an analyzed file.
        let seed = try XCTUnwrap(rows(model).first { $0.category != nil && $0.kind != nil })
        model.findSimilar(to: seed)
        await model.settle()
        XCTAssertNil(model.similarity.message)
        XCTAssertNil(model.errorMessage)
        let shown = rows(model)
        XCTAssertFalse(shown.isEmpty)
        XCTAssertFalse(shown.contains { $0.fileId == seed.fileId })
        let hits = model.similarity.hits
        XCTAssertEqual(hits.map(\.fileId), shown.map(\.fileId))
        XCTAssertEqual(hits.map(\.distance), hits.map(\.distance).sorted())

        // A slider change re-queries through the core without error.
        model.similarity.debounceNs = 0
        model.setSimilarityWeight(.pitch, 1)
        await model.settle()
        XCTAssertNil(model.similarity.message)
        XCTAssertFalse(rows(model).isEmpty)
    }

    func testExternalFileSeedReturnsRows() async throws {
        let model = try await indexedModel()
        let fixtures = try fixturesDir()
        let wav = try XCTUnwrap(
            FileManager.default.enumerator(atPath: fixtures)?.compactMap { $0 as? String }
                .first { $0.lowercased().hasSuffix(".wav") })
        model.findSimilar(externalFile: URL(fileURLWithPath: fixtures).appendingPathComponent(wav))
        await model.settle()
        XCTAssertNil(model.similarity.message)
        XCTAssertNil(model.errorMessage)
        XCTAssertFalse(rows(model).isEmpty)
        XCTAssertFalse(model.similarity.isLoading)
    }

    func testInvalidExternalFileShowsInlineMessage() async throws {
        let model = try await indexedModel()
        let bad = FileManager.default.temporaryDirectory.appendingPathComponent("bad-\(UUID().uuidString).wav")
        try Data("not audio".utf8).write(to: bad)
        addTeardownBlock { try? FileManager.default.removeItem(at: bad) }
        model.findSimilar(externalFile: bad)
        await model.settle()
        XCTAssertNotNil(model.similarity.message)
        XCTAssertNil(model.errorMessage, "no modal alert")
        XCTAssertEqual(model.resultCount, 0)
        XCTAssertFalse(model.similarity.isLoading)
    }

    func testLeavingSimilarityRestoresTextSearch() async throws {
        let model = try await indexedModel()
        let before = rows(model).map(\.fileId)
        XCTAssertFalse(before.isEmpty)
        model.findSimilar(to: try XCTUnwrap(rows(model).first))
        await model.settle()
        XCTAssertNotEqual(rows(model).map(\.fileId), before)
        model.exitSimilarity()
        await model.settle()
        XCTAssertFalse(model.similarity.isActive)
        XCTAssertEqual(rows(model).map(\.fileId), before)
    }
}
