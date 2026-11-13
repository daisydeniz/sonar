import AVFoundation
import XCTest
@testable import Sonar

/// Records what the facade asks the audio output to do.
@MainActor
final class FakePlayer: PreviewPlayer {
    var onFinished: (() -> Void)?
    private(set) var plays: [(clip: PreviewClip, loop: Bool, gainDb: Double?, pitch: Float)] = []
    private(set) var stops = 0
    private(set) var lastPlayInstant: ContinuousClock.Instant?
    private(set) var pitches: [Float] = []

    func play(_ clip: PreviewClip, loop: Bool, gainDb: Double?, pitchCents: Float) throws {
        plays.append((clip, loop, gainDb, pitchCents))
        lastPlayInstant = .now
    }
    func stop() { stops += 1 }
    func setPitch(cents: Float) { pitches.append(cents) }
}

/// Decoder returning tiny clips; per-file delays model slow decodes.
final class FakeDecoder: PreviewDecoding, @unchecked Sendable {
    private let lock = NSLock()
    private var requested: [Int64] = []
    var delays: [Int64: UInt64] = [:]
    var failing: Set<Int64> = []

    var requests: [Int64] { lock.withLock { requested } }

    func decode(_ row: FileRow) async throws -> PreviewClip {
        lock.withLock { requested.append(row.fileId) }
        if let d = delays[row.fileId] { try await Task.sleep(nanoseconds: d) }
        if failing.contains(row.fileId) { throw PreviewDecodeError.empty }
        return Self.clip(frames: 100)
    }

    static func clip(frames: Int) -> PreviewClip {
        let format = AVAudioFormat(standardFormatWithSampleRate: 44100, channels: 1)!
        let buffer = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: AVAudioFrameCount(frames))!
        buffer.frameLength = AVAudioFrameCount(frames)
        return PreviewClip(buffer: buffer)
    }
}

func previewRow(_ id: Int64, gain: Double? = nil) -> FileRow {
    FileRow(
        fileId: id, libraryId: 1, path: "/synthetic/\(id).wav", name: "row \(id)", folders: "",
        category: nil, kind: nil, bpm: nil, key: nil, durationMs: 100, sampleRate: 44100, channels: 1,
        previewGainDb: gain, favorite: false, hidden: false, tags: [])
}

@MainActor
final class AudioPreviewTests: XCTestCase {
    private var defaults: UserDefaults!

    override func setUp() async throws {
        defaults = UserDefaults(suiteName: "preview-tests-\(UUID().uuidString)")
    }

    private func make(
        decoder: FakeDecoder = FakeDecoder(), cache: PreviewCache = PreviewCache()
    ) -> (AudioPreview, FakePlayer, FakeDecoder) {
        let player = FakePlayer()
        let preview = AudioPreview(player: player, decoder: decoder, cache: cache, defaults: defaults)
        return (preview, player, decoder)
    }

    // MARK: Cache

    func testCacheEvictsLeastRecentlyUsedByCount() {
        let cache = PreviewCache(maxEntries: 3, maxBytes: 1 << 30)
        for id in 1...3 { cache.put(Int64(id), FakeDecoder.clip(frames: 10)) }
        _ = cache.get(1)  // 2 is now the oldest
        cache.put(4, FakeDecoder.clip(frames: 10))
        XCTAssertEqual(cache.ids, [3, 1, 4])
        XCTAssertFalse(cache.contains(2))
    }

    func testCacheByteCapEvictsButKeepsNewest() {
        let one = FakeDecoder.clip(frames: 1000).byteCount  // 4000 bytes
        let cache = PreviewCache(maxEntries: 10, maxBytes: one * 2)
        for id in 1...3 { cache.put(Int64(id), FakeDecoder.clip(frames: 1000)) }
        XCTAssertEqual(cache.ids, [2, 3])
        XCTAssertEqual(cache.totalBytes, one * 2)
        cache.put(9, FakeDecoder.clip(frames: 5000))  // alone over the cap
        XCTAssertEqual(cache.ids, [9])
    }

    // MARK: Selection, neighbours, latest-wins

    func testSelectPlaysAndPrefetchesNeighboursWithinBounds() async {
        let (preview, player, decoder) = make()
        preview.select(previewRow(5, gain: -3.5), neighbours: [4, 6, 3, 7].map { previewRow($0) })
        await preview.settle()
        XCTAssertEqual(player.plays.count, 1)
        XCTAssertEqual(player.plays.first?.gainDb, -3.5, "gain comes from previewGainDb")
        XCTAssertEqual(Set(decoder.requests), [3, 4, 5, 6, 7])
        XCTAssertTrue(preview.isPlaying)
        // Row 0 has no row below it: only the existing neighbours are requested.
        let (p2, _, d2) = make()
        p2.select(previewRow(0), neighbours: [1, 2].map { previewRow($0) })
        await p2.settle()
        XCTAssertEqual(Set(d2.requests), [0, 1, 2])
    }

    func testCachedSelectionPlaysWithoutDecoding() async {
        let (preview, player, decoder) = make()
        preview.select(previewRow(1), neighbours: [previewRow(2)])
        await preview.settle()
        preview.select(previewRow(2), neighbours: [previewRow(1)])
        XCTAssertEqual(player.plays.count, 2)
        XCTAssertEqual(decoder.requests.sorted(), [1, 2], "no second decode")
    }

    func testSlowDecodeOfOldSelectionDoesNotPlay() async {
        let decoder = FakeDecoder()
        decoder.delays[1] = 150_000_000
        let (preview, player, _) = make(decoder: decoder)
        preview.select(previewRow(1))
        preview.select(previewRow(2))
        await preview.settle()
        XCTAssertEqual(player.plays.count, 1)
        try? await Task.sleep(nanoseconds: 250_000_000)
        await preview.settle()
        XCTAssertEqual(player.plays.count, 1, "stale decode finished but must not play")
        XCTAssertTrue(preview.cache.contains(1), "it is still cached for later")
    }

    func testToggleStopsAndRestartsAndEmptySelectionStops() async {
        let (preview, player, _) = make()
        preview.select(previewRow(1))
        await preview.settle()
        preview.toggle()
        XCTAssertFalse(preview.isPlaying)
        preview.toggle()
        XCTAssertTrue(preview.isPlaying)
        XCTAssertEqual(player.plays.count, 2)
        preview.select(nil)
        XCTAssertFalse(preview.isPlaying)
        preview.toggle()
        XCTAssertEqual(player.plays.count, 2, "nothing selected, nothing to play")
    }

    func testAutoPlayOffOnlyPrefetchesAndPersists() async {
        let (preview, player, decoder) = make()
        preview.autoPlay = false
        XCTAssertEqual(defaults.object(forKey: AudioPreview.autoPlayKey) as? Bool, false)
        preview.select(previewRow(1))
        await preview.settle()
        XCTAssertTrue(player.plays.isEmpty)
        XCTAssertEqual(decoder.requests, [1])
        preview.toggle()
        XCTAssertEqual(player.plays.count, 1, "Space plays from cache")
        let reloaded = AudioPreview(player: FakePlayer(), defaults: defaults)
        XCTAssertFalse(reloaded.autoPlay)
    }

    func testReselectingSameRowDoesNotRestart() async {
        let (preview, player, _) = make()
        preview.select(previewRow(1))
        await preview.settle()
        preview.select(previewRow(1, gain: -2))  // table refresh after a poll
        XCTAssertEqual(player.plays.count, 1)
    }

    func testDecodeFailureIsQuiet() async {
        let decoder = FakeDecoder()
        decoder.failing = [1]
        let (preview, player, _) = make(decoder: decoder)
        preview.select(previewRow(1))
        await preview.settle()
        XCTAssertTrue(player.plays.isEmpty)
        XCTAssertFalse(preview.isPlaying)
        XCTAssertNotNil(preview.status)
        preview.select(previewRow(2))
        XCTAssertNil(preview.status)
    }

    func testPitchClampsAndLoopRestartsPlayback() async {
        let (preview, player, _) = make()
        preview.setPitch(semitones: 30)
        XCTAssertEqual(preview.pitchSemitones, 12)
        preview.setPitch(semitones: -30)
        XCTAssertEqual(preview.pitchSemitones, -12)
        preview.resetPitch()
        XCTAssertEqual(player.pitches.last, 0)
        preview.select(previewRow(1))
        await preview.settle()
        preview.loop = true
        XCTAssertEqual(player.plays.count, 2)
        XCTAssertEqual(player.plays.last?.loop, true)
    }

    // MARK: Latency

    func testCachedSelectToPlayLatencyUnderTenMs() async throws {
        let (preview, player, _) = make()
        preview.select(previewRow(1), neighbours: [previewRow(2)])
        await preview.settle()
        var worst = Duration.zero
        for i in 0..<20 {
            let id: Int64 = i % 2 == 0 ? 2 : 1
            let start = ContinuousClock.now
            preview.select(previewRow(id), neighbours: [previewRow(3 - id)])
            let end = try XCTUnwrap(player.lastPlayInstant)
            worst = max(worst, start.duration(to: end))
        }
        print("preview select->play latency (cached, fake player), worst of 20: \(worst)")
        XCTAssertLessThan(worst, .milliseconds(10))
    }

    /// Real engine smoke test: skipped when no output can be started.
    func testRealEngineSmoke() async throws {
        let player = EnginePlayer()
        var finished = false
        player.onFinished = { finished = true }
        let clip = FakeDecoder.clip(frames: 2205)  // 50 ms of silence
        let start = ContinuousClock.now
        do {
            try player.play(clip, loop: false, gainDb: 6, pitchCents: 200)
        } catch {
            throw XCTSkip("no audio output available: \(error)")
        }
        print("real engine first play call took \(start.duration(to: .now))")
        let done = await waitFor(timeout: 3) { finished }
        player.stop()
        XCTAssertTrue(done, "playback never reported completion")
    }
}
