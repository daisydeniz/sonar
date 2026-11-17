import XCTest
@testable import Sonar

/// Waveform region selection (DESIGN §4.4): the pure region math, region ->
/// playback range, region -> similarity request, clearing rules.
@MainActor
final class WaveformRegionTests: XCTestCase {
    // MARK: Pure model

    func testRegionNormalizesOrderAndClamps() throws {
        let r = try XCTUnwrap(WaveformRegion(from: 0.8, to: -0.2, duration: 2))
        XCTAssertEqual(r.start, 0)
        XCTAssertEqual(r.end, 0.8)
        XCTAssertTrue(r.contains(0.5))
        XCTAssertFalse(r.contains(0.9))
    }

    func testRegionShorterThan50msIsNil() {
        // 0.02 of 2 s = 40 ms
        XCTAssertNil(WaveformRegion(from: 0.5, to: 0.52, duration: 2))
        XCTAssertNotNil(WaveformRegion(from: 0.5, to: 0.53, duration: 2))  // 60 ms
        XCTAssertNil(WaveformRegion(from: 0.1, to: 0.9, duration: 0))
        XCTAssertNil(WaveformRegion(from: .nan, to: 0.9, duration: 1))
    }

    func testResizeKeepsMinimumLengthAndBounds() throws {
        let r = try XCTUnwrap(WaveformRegion(from: 0.25, to: 0.75, duration: 2))
        let moved = r.resized(.start, to: 0.9, duration: 2)  // dragged past the end
        XCTAssertEqual(moved.end, 0.75)
        XCTAssertEqual(moved.length(duration: 2), WaveformRegion.minDuration, accuracy: 1e-9)
        XCTAssertEqual(r.resized(.end, to: 3, duration: 2).end, 1)
        XCTAssertEqual(r.resized(.start, to: -1, duration: 2).start, 0)
    }

    func testReadoutShowsStartAndLength() throws {
        let r = try XCTUnwrap(WaveformRegion(from: 0.5, to: 0.75, duration: 2))
        XCTAssertEqual(r.readout(duration: 2), "0:01.000  +0.500 s")
    }

    // MARK: Region -> playback range

    func testPlaybackRangeAndStart() throws {
        let r = try XCTUnwrap(WaveformRegion(from: 0.25, to: 0.75, duration: 1))
        let p = try XCTUnwrap(r.playback(totalFrames: 44100, sampleRate: 44100))
        XCTAssertEqual(p.range, 11025..<33075)
        XCTAssertEqual(p.startAt, 0.25, accuracy: 1e-9)
        // A seek inside keeps its time, outside is clamped into the region.
        XCTAssertEqual(r.playback(totalFrames: 44100, sampleRate: 44100, startAt: 0.5)?.startAt ?? 0, 0.5, accuracy: 1e-9)
        XCTAssertEqual(r.playback(totalFrames: 44100, sampleRate: 44100, startAt: 0.9)?.startAt ?? 0, 0.75, accuracy: 1e-9)
        XCTAssertNil(r.playback(totalFrames: 0, sampleRate: 44100))
    }

    func testRegionPlaybackTimeLoopsInsideTheRegion() {
        // Region 0.25...0.75 s, started at its beginning.
        let t = { (e: Double, loop: Bool) in
            WaveformMath.playbackTime(elapsed: e, start: 0.25, from: 0.25, to: 0.75, loop: loop)
        }
        XCTAssertEqual(t(0.1, true), 0.35, accuracy: 1e-9)
        XCTAssertEqual(t(0.6, true), 0.35, accuracy: 1e-9)  // wrapped to the region start
        XCTAssertEqual(t(0.6, false), 0.75, accuracy: 1e-9)  // stops at the region end
        // The whole-sound variant is unchanged.
        XCTAssertEqual(WaveformMath.playbackTime(elapsed: 1.5, start: 0.5, duration: 1, loop: true), 0, accuracy: 1e-9)
    }

    func testPreviewPlaysTheRegionRange() async throws {
        let defaults = UserDefaults(suiteName: "region-tests-\(UUID().uuidString)")!
        defaults.set(false, forKey: AudioPreview.autoPlayKey)
        let player = FakePlayer()
        let decoder = FakeDecoder()
        decoder.frames = 44100
        let preview = AudioPreview(player: player, decoder: decoder, defaults: defaults)
        let row = previewRow(1)
        preview.select(row)
        preview.region = WaveformRegion(from: 0.25, to: 0.75, duration: 1)

        preview.toggle()  // space plays the region
        await preview.settle()
        XCTAssertEqual(player.ranges.last ?? nil, 11025..<33075)
        XCTAssertEqual(try XCTUnwrap(player.starts.last), 0.25, accuracy: 1e-9)

        preview.loop = true  // restarts, the region is the loop range
        XCTAssertEqual(player.ranges.last ?? nil, 11025..<33075)
        XCTAssertTrue(player.plays.last?.loop ?? false)

        preview.region = nil
        preview.playSelection()
        await preview.settle()
        XCTAssertNil(player.ranges.last ?? nil)
        XCTAssertEqual(try XCTUnwrap(player.starts.last), 0, accuracy: 1e-9)
    }

    func testPlaySelectionRestartsAPlayingRegion() async throws {
        let defaults = UserDefaults(suiteName: "region-tests-\(UUID().uuidString)")!
        let player = FakePlayer()
        let decoder = FakeDecoder()
        decoder.frames = 44100
        let preview = AudioPreview(player: player, decoder: decoder, defaults: defaults)
        preview.select(previewRow(1))  // auto-play on: whole clip
        await preview.settle()
        XCTAssertNil(player.ranges.last ?? nil)
        preview.region = WaveformRegion(from: 0.5, to: 0.9, duration: 1)
        preview.playSelection()
        await preview.settle()
        XCTAssertEqual(player.plays.count, 2)
        XCTAssertEqual(player.ranges.last ?? nil, 22050..<39690)
    }

    // MARK: Model wiring

    func testRegionReachesPreviewAndClearsOnNewSelection() {
        let model = AppModel()
        model.select(previewRow(1))
        model.waveform.setRegion(WaveformRegion(from: 0.2, to: 0.6, duration: 10))
        XCTAssertEqual(model.preview.region, model.waveform.region)
        XCTAssertNotNil(model.preview.region)
        model.select(previewRow(1))  // same file refreshed: keeps the region
        XCTAssertNotNil(model.waveform.region)
        model.select(previewRow(2))
        XCTAssertNil(model.waveform.region)
        XCTAssertNil(model.preview.region)
    }

    // MARK: Region -> similarity request

    private func row(_ id: Int64, durationMs: UInt64?) -> FileRow {
        FileRow(
            fileId: id, libraryId: 1, path: "/tmp/r-\(id).wav", name: "r-\(id)", folders: "", category: nil,
            kind: nil, bpm: nil, key: nil, durationMs: durationMs, sampleRate: nil, channels: nil,
            previewGainDb: nil, favorite: false, hidden: false, tags: [])
    }

    private func capture(_ model: AppModel) -> RequestBox {
        let box = RequestBox()
        model.similarity.searchOverride = { q in
            box.add(q)
            return SimilarPage(rows: [], hits: [], candidates: 0)
        }
        model.similarity.debounceNs = 0
        return box
    }

    func testFindSimilarWithRegionCarriesStartAndEndSeconds() async throws {
        let model = AppModel()
        let box = capture(model)
        model.select(row(7, durationMs: 2000))
        model.waveform.setRegion(WaveformRegion(from: 0.25, to: 0.75, duration: 2))
        model.findSimilarToSelection()
        await model.settle()
        let q = try XCTUnwrap(box.queries.last)
        XCTAssertEqual(q.seed, .file(fileId: 7))
        XCTAssertEqual(q.region, SimilarRegion(startS: 0.5, endS: 1.5))
        XCTAssertEqual(model.similarity.region, SimilarRegion(startS: 0.5, endS: 1.5))
    }

    func testFindSimilarWithoutRegionIsTheWholeFileRequest() async throws {
        let model = AppModel()
        let box = capture(model)
        model.select(row(7, durationMs: 2000))
        model.findSimilarToSelection()
        await model.settle()
        XCTAssertNil(try XCTUnwrap(box.queries.last).region)
        // A slider change re-queries with the same seed range.
        model.waveform.setRegion(WaveformRegion(from: 0.25, to: 0.75, duration: 2))
        model.setSimilarityWeight(.timbre, 0.5)
        await model.settle()
        XCTAssertNil(try XCTUnwrap(box.queries.last).region)
        // The empty fake page deselects: select again, then use the region.
        model.select(row(7, durationMs: 2000))
        model.waveform.setRegion(WaveformRegion(from: 0.25, to: 0.75, duration: 2))
        model.findSimilarToSelection()
        await model.settle()
        XCTAssertNotNil(try XCTUnwrap(box.queries.last).region)
        model.exitSimilarity()
        XCTAssertNil(model.similarity.region)
    }
}

/// Thread-safe log of the queries a fake searcher received.
private final class RequestBox: @unchecked Sendable {
    private let lock = NSLock()
    private var all: [SimilarQuery] = []
    var queries: [SimilarQuery] { lock.withLock { all } }
    func add(_ q: SimilarQuery) { lock.withLock { all.append(q) } }
}
