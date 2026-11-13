import AVFoundation
import Foundation

/// The audio output behind `AudioPreview` (a fake in tests).
@MainActor
protocol PreviewPlayer: AnyObject {
    /// Called when a non-looping sound played to its end or playback was
    /// interrupted by the system (e.g. output device change).
    var onFinished: (() -> Void)? { get set }
    /// Starts `clip` from the beginning (replacing what plays now).
    func play(_ clip: PreviewClip, loop: Bool, gainDb: Double?, pitchCents: Float) throws
    func stop()
    func setPitch(cents: Float)
    /// Pre-starts the output so the first play is fast (no-op by default).
    func warmUp()
}

extension PreviewPlayer {
    func warmUp() {}
}

/// AVAudioEngine graph: player -> time-pitch -> EQ (global gain only) -> main
/// mixer. The loudness gain (ADR-034) can exceed 0 dB, which player/mixer
/// volume (0...1) cannot do, so it is applied with the EQ's `globalGain`.
/// The engine is created on first play, so tests and headless runs never
/// touch audio hardware unless they play.
@MainActor
final class EnginePlayer: PreviewPlayer {
    var onFinished: (() -> Void)?

    private var engine: AVAudioEngine?
    private let player = AVAudioPlayerNode()
    private let timePitch = AVAudioUnitTimePitch()
    private let gain = AVAudioUnitEQ(numberOfBands: 1)
    private var connectedFormat: AVAudioFormat?
    private var playId = 0
    private var configObserver: NSObjectProtocol?

    /// Builds the graph for `format`, reconnecting when the clip format changes.
    private func prepare(for format: AVAudioFormat) {
        let engine = self.engine ?? makeEngine()
        guard connectedFormat != format else { return }
        player.stop()
        engine.connect(player, to: timePitch, format: format)
        engine.connect(timePitch, to: gain, format: format)
        engine.connect(gain, to: engine.mainMixerNode, format: format)
        connectedFormat = format
    }

    private func makeEngine() -> AVAudioEngine {
        let engine = AVAudioEngine()
        engine.attach(player)
        engine.attach(timePitch)
        engine.attach(gain)
        gain.bands[0].bypass = true
        self.engine = engine
        // Output device change: the engine stops itself; reconnect on next play.
        configObserver = NotificationCenter.default.addObserver(
            forName: .AVAudioEngineConfigurationChange, object: engine, queue: .main
        ) { [weak self] _ in
            MainActor.assumeIsolated { self?.configurationChanged() }
        }
        return engine
    }

    private func configurationChanged() {
        playId += 1
        player.stop()
        connectedFormat = nil
        onFinished?()
    }

    func play(_ clip: PreviewClip, loop: Bool, gainDb: Double?, pitchCents: Float) throws {
        prepare(for: clip.buffer.format)
        guard let engine else { return }
        player.stop()
        playId += 1
        let id = playId
        timePitch.pitch = pitchCents
        gain.globalGain = Float(min(max(gainDb ?? 0, -24), 24))
        player.scheduleBuffer(
            clip.buffer, at: nil, options: loop ? .loops : [], completionCallbackType: .dataPlayedBack
        ) { [weak self] _ in
            Task { @MainActor in self?.completed(id) }
        }
        if !engine.isRunning { try engine.start() }
        player.play()
    }

    private func completed(_ id: Int) {
        if id == playId { onFinished?() }
    }

    func stop() {
        playId += 1
        player.stop()
    }

    func setPitch(cents: Float) { timePitch.pitch = cents }

    /// Builds the graph and starts the output once (cold start costs ~150 ms),
    /// so the first preview after launch is as fast as the rest. Best effort.
    func warmUp() {
        guard let format = AVAudioFormat(standardFormatWithSampleRate: 44100, channels: 2) else { return }
        prepare(for: format)
        try? engine?.start()
    }
}
