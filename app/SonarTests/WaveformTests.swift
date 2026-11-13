import XCTest
@testable import Sonar

private func pk(_ a: Float, _ b: Float) -> WaveformPeak { WaveformPeak(min: a, max: b) }

final class WaveformMathTests: XCTestCase {
    func testRebucketWholeRangeMergesMinMax() {
        let peaks = [pk(-0.1, 0.2), pk(-0.5, 0.3), pk(-0.2, 0.9), pk(-0.3, 0.1)]
        let out = WaveformMath.rebucket(peaks, from: 0, to: 1, into: 2)
        XCTAssertEqual(out, [pk(-0.5, 0.3), pk(-0.3, 0.9)])
    }

    func testRebucketZoomedSliceAndUpsampleNeverHasHoles() {
        let peaks = (0..<8).map { pk(-Float($0) / 8, Float($0) / 8) }
        // Visible 25%..50% = buckets 2,3; 8 columns -> each repeats its bucket.
        let out = WaveformMath.rebucket(peaks, from: 0.25, to: 0.5, into: 8)
        XCTAssertEqual(out.count, 8)
        XCTAssertEqual(out.first, peaks[2])
        XCTAssertEqual(out.last, peaks[3])
    }

    func testRebucketDegenerateInputs() {
        XCTAssertTrue(WaveformMath.rebucket([], from: 0, to: 1, into: 4).isEmpty)
        XCTAssertTrue(WaveformMath.rebucket([pk(0, 1)], from: 0.5, to: 0.5, into: 4).isEmpty)
        XCTAssertTrue(WaveformMath.rebucket([pk(0, 1)], from: 0, to: 1, into: 0).isEmpty)
        XCTAssertEqual(WaveformMath.rebucket([pk(0, 1)], from: -1, to: 2, into: 3).count, 3)
    }

    func testPathSegmentsSpanPeaksAndHaveMinimumHeight() {
        let path = WaveformMath.path(columns: [pk(-1, 1), pk(0, 0)], size: CGSize(width: 20, height: 100))
        let box = path.boundingBoxOfPath
        XCTAssertEqual(box.minY, 0, accuracy: 0.01)
        XCTAssertEqual(box.maxY, 100, accuracy: 0.01)
        XCTAssertEqual(box.minX, 5, accuracy: 0.01)  // first column centre
        XCTAssertEqual(box.maxX, 15, accuracy: 0.01)
        let silent = WaveformMath.path(columns: [pk(0, 0)], size: CGSize(width: 10, height: 100))
        XCTAssertEqual(silent.boundingBoxOfPath.height, 1, accuracy: 0.01)
        XCTAssertTrue(WaveformMath.path(columns: [], size: CGSize(width: 10, height: 10)).isEmpty)
    }

    func testViewportMapping() {
        let v = WaveformViewport(zoom: 4, start: 0.5)
        XCTAssertEqual(v.span, 0.25, accuracy: 1e-9)
        XCTAssertEqual(v.fraction(atX: 0, width: 200), 0.5, accuracy: 1e-9)
        XCTAssertEqual(v.fraction(atX: 100, width: 200), 0.625, accuracy: 1e-9)
        XCTAssertEqual(v.fraction(atX: 999, width: 200), 0.75, accuracy: 1e-9)
        XCTAssertEqual(v.x(forFraction: 0.625, width: 200), 100, accuracy: 1e-9)
        XCTAssertLessThan(v.x(forFraction: 0.1, width: 200), 0)
    }

    func testViewportClampsAndZoomKeepsAnchor() {
        XCTAssertEqual(WaveformViewport(zoom: 1000, start: 5).zoom, WaveformViewport.maxZoom)
        XCTAssertEqual(WaveformViewport(zoom: 0.1, start: 0.3).start, 0)
        let v = WaveformViewport().zoomed(by: 4, anchor: 0.25)
        XCTAssertEqual(v.zoom, 4, accuracy: 1e-9)
        XCTAssertEqual(v.fraction(atX: 25, width: 100), 0.25, accuracy: 1e-9)  // pivot fixed
        let out = v.zoomed(by: 0.001, anchor: 0.5)
        XCTAssertEqual(out, WaveformViewport())  // fully zoomed out resets scroll
        XCTAssertEqual(v.panned(by: 5).end, 1, accuracy: 1e-9)
        XCTAssertEqual(v.panned(by: -5).start, 0, accuracy: 1e-9)
        XCTAssertEqual(WaveformViewport().panned(byPoints: 50, width: 100).start, 0)
        XCTAssertEqual(WaveformViewport(zoom: 4, start: 0).panned(byPoints: 100, width: 100).start, 0.25, accuracy: 1e-9)
    }

    func testPlaybackTimeAndLoopWrap() {
        XCTAssertEqual(WaveformMath.playbackTime(elapsed: 0.5, start: 0, duration: 2, loop: false), 0.5, accuracy: 1e-9)
        XCTAssertEqual(WaveformMath.playbackTime(elapsed: 5, start: 0, duration: 2, loop: false), 2, accuracy: 1e-9)
        // Loop: wraps modulo the duration.
        XCTAssertEqual(WaveformMath.playbackTime(elapsed: 2.5, start: 0, duration: 2, loop: true), 0.5, accuracy: 1e-9)
        // Seek to 1.5 s, loop: first pass is 0.5 s, then whole sound.
        XCTAssertEqual(WaveformMath.playbackTime(elapsed: 0.25, start: 1.5, duration: 2, loop: true), 1.75, accuracy: 1e-9)
        XCTAssertEqual(WaveformMath.playbackTime(elapsed: 1.0, start: 1.5, duration: 2, loop: true), 0.5, accuracy: 1e-9)
        XCTAssertEqual(WaveformMath.playbackTime(elapsed: 1, start: 0, duration: 0, loop: true), 0)
    }
}

final class FakeWaveformProvider: WaveformProviding, @unchecked Sendable {
    private let lock = NSLock()
    private var calls: [Int64] = []
    var delays: [Int64: UInt64] = [:]
    var failing: Set<Int64> = []

    var requests: [Int64] { lock.withLock { calls } }

    func peaks(fileId: Int64, buckets: Int) async throws -> [WaveformPeak] {
        lock.withLock { calls.append(fileId) }
        if let d = delays[fileId] { try await Task.sleep(nanoseconds: d) }
        if failing.contains(fileId) { throw PreviewDecodeError.empty }
        return [pk(-Float(fileId), Float(fileId))]
    }
}

@MainActor
final class WaveformModelTests: XCTestCase {
    func testLoadsAndCaches() async {
        let provider = FakeWaveformProvider()
        let model = WaveformModel(provider: provider)
        model.load(fileId: 1)
        XCTAssertEqual(model.state, .loading)
        await model.settle()
        XCTAssertEqual(model.state, .ready([pk(-1, 1)]))
        model.load(fileId: 2)
        await model.settle()
        model.load(fileId: 1)  // cached: instant, no new request
        XCTAssertEqual(model.state, .ready([pk(-1, 1)]))
        XCTAssertEqual(provider.requests, [1, 2])
    }

    func testLatestSelectionWins() async {
        let provider = FakeWaveformProvider()
        provider.delays[1] = 100_000_000
        let model = WaveformModel(provider: provider)
        model.load(fileId: 1)
        model.load(fileId: 2)
        await model.settle()
        try? await Task.sleep(nanoseconds: 200_000_000)
        XCTAssertEqual(model.state, .ready([pk(-2, 2)]))
    }

    func testFailureAndNilSelection() async {
        let provider = FakeWaveformProvider()
        provider.failing = [3]
        let model = WaveformModel(provider: provider)
        model.load(fileId: 3)
        await model.settle()
        XCTAssertEqual(model.state, .failed)
        model.load(fileId: nil)
        XCTAssertEqual(model.state, .empty)
        let noProvider = WaveformModel()
        noProvider.load(fileId: 1)
        XCTAssertEqual(noProvider.state, .failed)
    }

    func testSelectionResetsZoom() async {
        let model = WaveformModel(provider: FakeWaveformProvider())
        model.load(fileId: 1)
        await model.settle()
        model.zoom(by: 4)
        XCTAssertTrue(model.viewport.isZoomed)
        model.load(fileId: 2)
        XCTAssertFalse(model.viewport.isZoomed)
    }

    func testCacheEvictsLeastRecentlyUsed() {
        let cache = WaveformCache(capacity: 2)
        cache.put(1, [pk(0, 1)])
        cache.put(2, [pk(0, 1)])
        _ = cache.get(1)
        cache.put(3, [pk(0, 1)])
        XCTAssertNotNil(cache.get(1))
        XCTAssertNil(cache.get(2))
        XCTAssertEqual(cache.count, 2)
    }
}

/// Player reporting a fixed time; records seeks.
@MainActor
private final class SeekPlayer: PreviewPlayer {
    var onFinished: (() -> Void)?
    var time: TimeInterval? = 0.5
    private(set) var starts: [TimeInterval] = []
    func play(_ clip: PreviewClip, loop: Bool, gainDb: Double?, pitchCents: Float) throws { starts.append(0) }
    func play(_ clip: PreviewClip, loop: Bool, gainDb: Double?, pitchCents: Float, startAt: TimeInterval) throws {
        starts.append(startAt)
    }
    func stop() {}
    func setPitch(cents: Float) {}
    func playbackTime() -> TimeInterval? { time }
}

@MainActor
final class PlayheadSeekTests: XCTestCase {
    func testFractionAndSeek() async {
        let player = SeekPlayer()
        let decoder = FakeDecoder()  // 100 frames @ 44.1 kHz
        let preview = AudioPreview(
            player: player, decoder: decoder,
            defaults: UserDefaults(suiteName: "seek-tests-\(UUID().uuidString)")!)
        XCTAssertNil(preview.playbackFraction())
        preview.select(previewRow(1))
        await preview.settle()
        XCTAssertTrue(preview.isPlaying)
        let duration = 100.0 / 44100
        player.time = duration / 2
        XCTAssertEqual(preview.playbackFraction() ?? -1, 0.5, accuracy: 1e-6)
        preview.seek(toFraction: 0.25)
        XCTAssertEqual(player.starts.last ?? -1, duration * 0.25, accuracy: 1e-9)
        preview.stop()
        XCTAssertNil(preview.playbackFraction())
    }
}
