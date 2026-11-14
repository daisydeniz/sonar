import AVFoundation
import XCTest
@testable import Sonar

/// `EnginePlayer` on an engine that renders offline (no audio hardware): the
/// real node graph runs and its output can be inspected.
@MainActor
final class EnginePlayerTests: XCTestCase {
    private var engine: AVAudioEngine?

    private func makeOfflinePlayer() -> EnginePlayer {
        EnginePlayer { [unowned self] engine in
            let out = AVAudioFormat(standardFormatWithSampleRate: 48000, channels: 2)!
            try? engine.enableManualRenderingMode(.offline, format: out, maximumFrameCount: 4096)
            self.engine = engine
        }
    }

    private func format(_ rate: Double, _ channels: AVAudioChannelCount) -> AVAudioFormat {
        AVAudioFormat(standardFormatWithSampleRate: rate, channels: channels)!
    }

    /// Half a second of a 440 Hz sine (or silence with `amplitude` 0).
    private func tone(_ format: AVAudioFormat, amplitude: Float = 0.5) -> PreviewClip {
        let frames = Int(format.sampleRate / 2)
        let buffer = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: AVAudioFrameCount(frames))!
        buffer.frameLength = AVAudioFrameCount(frames)
        for ch in 0..<Int(format.channelCount) {
            for i in 0..<frames {
                buffer.floatChannelData![ch][i] =
                    amplitude * sin(2 * .pi * 440 * Float(i) / Float(format.sampleRate))
            }
        }
        return PreviewClip(buffer: buffer)
    }

    /// Loudest sample of the next `blocks` rendered blocks (4096 frames each).
    private func renderPeak(blocks: Int = 4) throws -> Float {
        let engine = try XCTUnwrap(self.engine)
        let buffer = try XCTUnwrap(
            AVAudioPCMBuffer(
                pcmFormat: engine.manualRenderingFormat,
                frameCapacity: engine.manualRenderingMaximumFrameCount))
        var peak: Float = 0
        for _ in 0..<blocks {
            let status = try engine.renderOffline(buffer.frameCapacity, to: buffer)
            XCTAssertEqual(status, .success)
            guard let data = buffer.floatChannelData else { continue }
            for ch in 0..<Int(buffer.format.channelCount) {
                for i in 0..<Int(buffer.frameLength) { peak = max(peak, abs(data[ch][i])) }
            }
        }
        return peak
    }

    /// Clips of different sample rates and channel counts play one after the
    /// other on the running engine (the crash of 510f468), each is heard, and
    /// the number of chains stays capped.
    func testFormatChangesPlayOnTheRunningEngine() throws {
        let player = makeOfflinePlayer()
        let formats = [
            format(44100, 1), format(48000, 2), format(96000, 2),
            format(22050, 1), format(88200, 1), format(44100, 2),
        ]
        XCTAssertGreaterThan(formats.count, EnginePlayer.maxChains)
        for (i, format) in formats.enumerated() {
            let pitch: Float = i % 2 == 0 ? 0 : 300
            try player.play(tone(format), loop: true, gainDb: nil, pitchCents: pitch)
            XCTAssertEqual(engine?.isRunning, true)
            XCTAssertGreaterThan(try renderPeak(), 0.1, "\(format) is silent")
            XCTAssertEqual(player.chainCount, min(i + 1, EnginePlayer.maxChains))
        }
        // The first format lost its chain to the cap: it gets a new one.
        try player.play(tone(formats[0]), loop: true, gainDb: nil, pitchCents: 0)
        XCTAssertGreaterThan(try renderPeak(), 0.1)
        XCTAssertEqual(player.chainCount, EnginePlayer.maxChains)
        // A format that still has a chain reuses it.
        try player.play(tone(formats[5]), loop: true, gainDb: nil, pitchCents: 0)
        XCTAssertGreaterThan(try renderPeak(), 0.1)
        XCTAssertEqual(player.chainCount, EnginePlayer.maxChains)
    }

    /// Switching to a clip of another format silences the previous sound,
    /// which plays on a different chain.
    func testSwitchingFormatStopsThePreviousSound() throws {
        let player = makeOfflinePlayer()
        try player.play(tone(format(44100, 2)), loop: true, gainDb: nil, pitchCents: 0)
        XCTAssertGreaterThan(try renderPeak(), 0.1)
        try player.play(tone(format(48000, 1), amplitude: 0), loop: true, gainDb: nil, pitchCents: 0)
        _ = try renderPeak(blocks: 1)  // converter tail of the stopped sound
        XCTAssertLessThan(try renderPeak(), 0.001)
        XCTAssertNotNil(player.playbackTime())
        player.stop()
        XCTAssertNil(player.playbackTime())
    }

    /// An output device change ends the current sound and the next play
    /// starts over with fresh chains.
    func testDeviceChangeRebuildsTheChains() throws {
        let player = makeOfflinePlayer()
        var finished = 0
        player.onFinished = { finished += 1 }
        try player.play(tone(format(44100, 2)), loop: true, gainDb: nil, pitchCents: 0)
        let nodesWithOneChain = try XCTUnwrap(engine).attachedNodes.count
        try player.play(tone(format(48000, 2)), loop: true, gainDb: nil, pitchCents: 0)
        XCTAssertEqual(player.chainCount, 2)
        NotificationCenter.default.post(name: .AVAudioEngineConfigurationChange, object: engine)
        XCTAssertEqual(finished, 1)
        try player.play(tone(format(96000, 1)), loop: true, gainDb: 6, pitchCents: -200)
        XCTAssertEqual(player.chainCount, 1)
        // The old chains' nodes are really gone from the engine.
        XCTAssertEqual(engine?.attachedNodes.count, nodesWithOneChain)
        XCTAssertGreaterThan(try renderPeak(), 0.1)
    }

    /// Real output: format changes in a row must not raise (skipped when no
    /// output can be started). Silent clips, so nothing is heard.
    func testRealEngineFormatChangesSmoke() async throws {
        let player = EnginePlayer()
        let formats = [
            format(44100, 2), format(48000, 1), format(96000, 2), format(44100, 1),
            format(22050, 2), format(48000, 2), format(44100, 2),
        ]
        for (i, format) in formats.enumerated() {
            do {
                try player.play(
                    tone(format, amplitude: 0), loop: i % 2 == 0, gainDb: 3,
                    pitchCents: i % 2 == 0 ? 0 : 500)
            } catch {
                throw XCTSkip("no audio output available: \(error)")
            }
            // The first render cycle can take a while on a busy machine.
            for _ in 0..<25 where player.playbackTime() == nil {
                try await Task.sleep(nanoseconds: 20_000_000)
            }
            XCTAssertNotNil(player.playbackTime(), "\(format) is not playing")
        }
        player.stop()
        XCTAssertLessThanOrEqual(player.chainCount, EnginePlayer.maxChains)
    }
}
