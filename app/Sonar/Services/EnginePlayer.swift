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
    /// Like `play` but starting `startAt` seconds into the clip (seek).
    func play(_ clip: PreviewClip, loop: Bool, gainDb: Double?, pitchCents: Float, startAt: TimeInterval) throws
    /// Seconds into the clip being heard now (loop-wrapped); nil when idle.
    func playbackTime() -> TimeInterval?
}

extension PreviewPlayer {
    func warmUp() {}
    func playbackTime() -> TimeInterval? { nil }
    func play(
        _ clip: PreviewClip, loop: Bool, gainDb: Double?, pitchCents: Float, startAt: TimeInterval
    ) throws {
        try play(clip, loop: loop, gainDb: gainDb, pitchCents: pitchCents)
    }
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
    private var startOffset: TimeInterval = 0
    private var playingLoop = false
    private var playingDuration: TimeInterval = 0

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
        try play(clip, loop: loop, gainDb: gainDb, pitchCents: pitchCents, startAt: 0)
    }

    func play(
        _ clip: PreviewClip, loop: Bool, gainDb: Double?, pitchCents: Float, startAt: TimeInterval
    ) throws {
        prepare(for: clip.buffer.format)
        guard let engine else { return }
        player.stop()
        playId += 1
        let id = playId
        setPitch(cents: pitchCents)
        gain.globalGain = Float(min(max(gainDb ?? 0, -24), 24))
        let rate = clip.buffer.format.sampleRate
        let total = Int(clip.buffer.frameLength)
        let startFrame = rate > 0 ? min(max(Int(startAt * rate), 0), max(total - 1, 0)) : 0
        let tail = startFrame > 0 ? Self.tail(of: clip.buffer, from: startFrame) : nil
        startOffset = tail != nil ? Double(startFrame) / rate : 0
        playingLoop = loop
        playingDuration = clip.duration
        if let tail {
            // Seek: play the tail first (then the whole clip again when looping).
            player.scheduleBuffer(
                tail, at: nil, options: [], completionCallbackType: .dataPlayedBack
            ) { [weak self] _ in
                if !loop { Task { @MainActor in self?.completed(id) } }
            }
            if loop { player.scheduleBuffer(clip.buffer, at: nil, options: .loops, completionHandler: nil) }
        } else {
            player.scheduleBuffer(
                clip.buffer, at: nil, options: loop ? .loops : [], completionCallbackType: .dataPlayedBack
            ) { [weak self] _ in
                Task { @MainActor in self?.completed(id) }
            }
        }
        if !engine.isRunning { try engine.start() }
        player.play()
    }

    private func completed(_ id: Int) {
        if id == playId { onFinished?() }
    }

    /// Copy of `buffer` from frame `from` to its end (seek start).
    private static func tail(of buffer: AVAudioPCMBuffer, from: Int) -> AVAudioPCMBuffer? {
        let count = Int(buffer.frameLength) - from
        guard count > 0, let src = buffer.floatChannelData,
            let out = AVAudioPCMBuffer(pcmFormat: buffer.format, frameCapacity: AVAudioFrameCount(count)),
            let dst = out.floatChannelData
        else { return nil }
        for ch in 0..<Int(buffer.format.channelCount) {
            dst[ch].update(from: src[ch] + from, count: count)
        }
        out.frameLength = AVAudioFrameCount(count)
        return out
    }

    func playbackTime() -> TimeInterval? {
        guard player.isPlaying, let node = player.lastRenderTime,
            let t = player.playerTime(forNodeTime: node), t.sampleRate > 0
        else { return nil }
        return WaveformMath.playbackTime(
            elapsed: Double(t.sampleTime) / t.sampleRate, start: startOffset,
            duration: playingDuration, loop: playingLoop)
    }

    func stop() {
        playId += 1
        player.stop()
    }

    /// Sets the pitch shift; at 0 the time-pitch unit is bypassed (no added latency).
    func setPitch(cents: Float) {
        timePitch.pitch = cents
        timePitch.bypass = cents == 0
    }

    /// Builds the graph and starts the output once (cold start costs ~150 ms),
    /// so the first preview after launch is as fast as the rest. Best effort.
    func warmUp() {
        guard let format = AVAudioFormat(standardFormatWithSampleRate: 44100, channels: 2) else { return }
        prepare(for: format)
        try? engine?.start()
    }
}
