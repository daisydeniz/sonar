import XCTest
@testable import Sonar

/// Drives a `sonar fixtures` library end to end through the generated UniFFI
/// bindings (T5.6 acceptance): add -> index -> search -> favourite -> search
/// again -> waveform peaks -> preview PCM, plus error mapping.
///
/// The fixtures directory comes from `SONAR_FIXTURES_DIR`, which
/// `scripts/check.sh` sets (as `TEST_RUNNER_SONAR_FIXTURES_DIR`, the prefix
/// is stripped by xcodebuild). Run from Xcode by hand without it, the test
/// skips. Engine calls are blocking, so they run on a background thread via
/// `Task.detached`, as the app does.
/// Thrown (a test failure, not a skip) when check.sh requires fixtures but
/// `SONAR_FIXTURES_DIR` did not reach the test process.
private struct FixturesMissing: Error {}

final class EngineBridgeTests: XCTestCase {
    private var home: URL!

    override func setUpWithError() throws {
        home = FileManager.default.temporaryDirectory
            .appendingPathComponent("sonar-engine-test-\(UUID().uuidString)")
    }

    override func tearDownWithError() throws {
        try? FileManager.default.removeItem(at: home)
    }

    private func fixturesDir() throws -> String {
        let env = ProcessInfo.processInfo.environment
        guard let dir = env["SONAR_FIXTURES_DIR"], !dir.isEmpty else {
            // check.sh sets SONAR_REQUIRE_FIXTURES=1, so a broken env
            // forwarding fails there instead of silently skipping.
            if env["SONAR_REQUIRE_FIXTURES"] == "1" {
                throw FixturesMissing()
            }
            throw XCTSkip("SONAR_FIXTURES_DIR not set (scripts/check.sh sets it)")
        }
        return dir
    }

    /// Polls until the indexing job has ended (progress is polled, ADR-032).
    private func waitUntilIdle(_ engine: SonarEngine, timeout: TimeInterval = 180) async throws {
        let deadline = Date().addingTimeInterval(timeout)
        while engine.indexingProgress().running {
            XCTAssertLessThan(Date(), deadline, "indexing did not finish in \(timeout) s")
            if Date() >= deadline { return }
            try await Task.sleep(nanoseconds: 50_000_000)
        }
    }

    func testFixturesLibraryEndToEnd() async throws {
        let fixtures = try fixturesDir()
        let homePath = home.path

        let engine = try await Task.detached { try SonarEngine.open(home: homePath) }.value

        // add: the scan finds the files, searchable by name right away.
        let added = try await Task.detached { try engine.addLibrary(path: fixtures) }.value
        XCTAssertGreaterThan(added.scan.added, 100)
        XCTAssertTrue(added.library.online)
        XCTAssertEqual(try engine.libraries().count, 1)

        // index (no CLAP model needed), polled.
        XCTAssertTrue(try engine.startIndexing(includeEmbeddings: false))
        try await waitUntilIdle(engine)
        XCTAssertFalse(engine.indexingProgress().running)

        // search: text mode, then the default (hybrid, text-only fallback).
        let page = try await Task.detached {
            try engine.search(query: SearchQuery(text: "hit", mode: .text))
        }.value
        XCTAssertGreaterThanOrEqual(page.rows.count, 12)
        XCTAssertEqual(page.mode, .text)
        let hit = try XCTUnwrap(page.rows.first)
        XCTAssertFalse(hit.favorite)
        XCTAssertNotNil(hit.durationMs, "indexed files know their duration")
        let hybrid = try engine.search(query: SearchQuery(text: "hit"))
        XCTAssertFalse(hybrid.rows.isEmpty)

        // favourite, then search again with the favourites filter.
        XCTAssertEqual(try engine.setFavorite(fileIds: [hit.fileId], on: true), 1)
        let favourites = try engine.search(query: SearchQuery(
            text: "hit", mode: .text, filters: SearchFilters(favoritesOnly: true)))
        XCTAssertEqual(favourites.rows.map(\.fileId), [hit.fileId])
        XCTAssertEqual(favourites.rows.first?.favorite, true)
        XCTAssertEqual(try engine.row(fileId: hit.fileId)?.favorite, true)

        // paging.
        let first = try engine.search(query: SearchQuery(text: "hit", mode: .text, limit: 5))
        XCTAssertEqual(first.rows.count, 5)
        XCTAssertTrue(first.hasMore)
        let second = try engine.search(query: SearchQuery(text: "hit", mode: .text, offset: 5, limit: 5))
        XCTAssertTrue(Set(first.rows.map(\.fileId)).isDisjoint(with: second.rows.map(\.fileId)))

        // waveform peaks.
        let peaks = try await Task.detached { try engine.waveformPeaks(fileId: hit.fileId, buckets: 200) }.value
        XCTAssertEqual(peaks.count, 200)
        XCTAssertTrue(peaks.allSatisfy { $0.min <= $0.max && abs($0.min) <= 1 && abs($0.max) <= 1 })
        XCTAssertTrue(peaks.contains { $0.max - $0.min > 0.01 }, "a one-shot is not silent")

        // preview PCM of an Ogg Vorbis file (AVFoundation cannot play it).
        let clips = try engine.search(query: SearchQuery(text: "clip", mode: .text, limit: 100))
        let ogg = try XCTUnwrap(clips.rows.first { $0.path.hasSuffix(".ogg") })
        let pcm = try await Task.detached { try engine.previewPcm(fileId: ogg.fileId) }.value
        XCTAssertGreaterThan(pcm.sampleRate, 0)
        XCTAssertEqual(Int(pcm.channels), 1)
        XCTAssertFalse(pcm.samples.isEmpty)
        XCTAssertTrue(pcm.samples.allSatisfy { abs($0) <= 1.0001 })

        // errors throw, they do not trap.
        XCTAssertThrowsError(try engine.setFavorite(fileIds: [-1], on: true)) { error in
            guard case FfiError.NotFound = error else { return XCTFail("unexpected \(error)") }
        }
        XCTAssertThrowsError(try engine.waveformPeaks(fileId: hit.fileId, buckets: 0)) { error in
            guard case FfiError.InvalidInput = error else { return XCTFail("unexpected \(error)") }
        }
    }
}
