import AVFoundation
import XCTest
@testable import Sonar

/// Decodes real fixture files (WAV via AVAudioFile, Ogg via the core's
/// `previewPcm` fallback). Skips without `SONAR_FIXTURES_DIR`.
final class PreviewDecodingTests: XCTestCase {
    func testPcmConversionDeinterleavesAndKeepsRate() throws {
        let pcm = PreviewPcm(sampleRate: 22050, channels: 2, samples: [1, -1, 0.5, -0.5], truncated: false)
        let clip = try PreviewDecoder.clip(from: pcm)
        XCTAssertEqual(clip.buffer.frameLength, 2)
        XCTAssertEqual(clip.buffer.format.sampleRate, 22050)
        XCTAssertEqual(clip.buffer.floatChannelData?[0][1], 0.5)
        XCTAssertEqual(clip.buffer.floatChannelData?[1][0], -1)
        XCTAssertThrowsError(try PreviewDecoder.clip(from: PreviewPcm(sampleRate: 44100, channels: 1, samples: [], truncated: false)))
    }

    func testDecodesWavAndOggFixtures() async throws {
        let env = ProcessInfo.processInfo.environment
        guard let dir = env["SONAR_FIXTURES_DIR"], !dir.isEmpty else {
            throw XCTSkip("SONAR_FIXTURES_DIR not set (scripts/check.sh sets it)")
        }
        let home = FileManager.default.temporaryDirectory
            .appendingPathComponent("sonar-preview-test-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: home) }
        let homePath = home.path
        let engine = try await Task.detached { try SonarEngine.open(home: homePath) }.value
        _ = try await Task.detached { try engine.addLibrary(path: dir) }.value
        let rows = try engine.search(query: SearchQuery(text: "hit", mode: .text, limit: 200)).rows
            + engine.search(query: SearchQuery(text: "clip", mode: .text, limit: 200)).rows
        let wav = try XCTUnwrap(rows.first { $0.path.hasSuffix(".wav") })
        let ogg = try XCTUnwrap(rows.first { $0.path.hasSuffix(".ogg") })

        let decoder = CoreDecoder(engine: engine)
        let wavClip = try await decoder.decode(wav)
        XCTAssertGreaterThan(wavClip.buffer.frameLength, 100)
        XCTAssertGreaterThan(wavClip.buffer.format.sampleRate, 8000)
        XCTAssertLessThan(wavClip.duration, PreviewDecoder.maxSeconds)
        XCTAssertNotNil(try? PreviewDecoder.readWithAVFoundation(URL(fileURLWithPath: wav.path)))

        // Newer macOS opens Ogg Vorbis natively; older ones do not. Either way
        // the decoder must produce a clip.
        let oggClip = try await decoder.decode(ogg)
        XCTAssertGreaterThan(oggClip.buffer.frameLength, 100)

        // Force the core fallback (a path AVFoundation cannot open).
        let id = ogg.fileId
        let forced = try await Task.detached {
            try PreviewDecoder.decode(
                url: URL(fileURLWithPath: "/nonexistent.ogg"), fallback: { try engine.previewPcm(fileId: id) })
        }.value
        XCTAssertGreaterThan(forced.buffer.frameLength, 100)
        XCTAssertEqual(forced.buffer.format.channelCount, 1)
        XCTAssertGreaterThan(forced.buffer.format.sampleRate, 8000)

        // A missing file with no fallback is an error value, not a crash.
        struct NoFallback: Error {}
        XCTAssertThrowsError(try PreviewDecoder.decode(
            url: URL(fileURLWithPath: "/nonexistent.wav"), fallback: { throw NoFallback() }))
    }
}
