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

/// AVAudioEngine graph with one *chain* per clip format: player -> time-pitch
/// -> EQ (global gain only) -> its own input of the main mixer, which converts
/// sample rate and channel count. The loudness gain (ADR-034) can exceed 0 dB,
/// which player/mixer volume (0...1) cannot do, so it is applied with the EQ's
/// `globalGain`. The engine is created on first play, so tests and headless
/// runs never touch audio hardware unless they play.
///
/// Why chains: rewiring *existing* nodes of a running engine to a new format
/// (other sample rate or channel count) raises an Objective-C exception
/// (-10868, format not supported) that Swift cannot catch, and stopping the
/// engine to rewire costs ~47 ms per format change. A chain is wired once and
/// never rewired; a new format gets new nodes, which is safe on a running
/// engine (measured 2026-11-14: ~1 ms to add, 0 exceptions in 1300 tries;
/// format switches ~12 ms, same as within one format).
@MainActor
final class EnginePlayer: PreviewPlayer {
    /// Player, pitch and gain nodes wired for exactly one format.
    private final class Chain {
        let format: AVAudioFormat
        let player = AVAudioPlayerNode()
        let timePitch = AVAudioUnitTimePitch()
        let gain = AVAudioUnitEQ(numberOfBands: 1)
        var lastUsed = 0

        init(format: AVAudioFormat) {
            self.format = format
            gain.bands[0].bypass = true
        }
        var nodes: [AVAudioNode] { [player, timePitch, gain] }
    }

    /// Chains kept wired at once; the least recently used one is removed
    /// beyond this. Typical libraries need 4 (44.1/48 kHz x mono/stereo).
    static let maxChains = 4

    var onFinished: (() -> Void)?

    private var engine: AVAudioEngine?
    private let engineSetup: ((AVAudioEngine) -> Void)?
    private var chains: [Chain] = []
    /// The chain that plays (or played last); pitch and time come from it.
    private var active: Chain?
    private var useTick = 0
    /// Set on an output device change: all chains are rebuilt on the next play.
    private var graphIsStale = false
    private var playId = 0
    private var configObserver: NSObjectProtocol?
    private var startOffset: TimeInterval = 0
    private var playingLoop = false
    private var playingDuration: TimeInterval = 0

    /// `engineSetup` runs once on the new engine before anything is attached
    /// (tests use it to render offline instead of to the audio hardware).
    init(engineSetup: ((AVAudioEngine) -> Void)? = nil) {
        self.engineSetup = engineSetup
    }

    /// Number of chains wired now (tests).
    var chainCount: Int { chains.count }

    /// The chain for `format`, adding one (and dropping the least recently
    /// used beyond `maxChains`) when the format is new.
    private func chain(for format: AVAudioFormat) -> Chain {
        let engine = self.engine ?? makeEngine()
        if graphIsStale {
            // The engine stopped itself and the output format may differ:
            // start over with fresh chains; `play` restarts the engine.
            if engine.isRunning { engine.stop() }
            chains.forEach { remove($0, from: engine) }
            chains.removeAll()
            active = nil
            graphIsStale = false
        }
        useTick += 1
        if let chain = chains.first(where: { $0.format == format }) {
            chain.lastUsed = useTick
            return chain
        }
        while chains.count >= Self.maxChains,
            let oldest = chains.indices.min(by: { chains[$0].lastUsed < chains[$1].lastUsed })
        {
            let dropped = chains.remove(at: oldest)
            remove(dropped, from: engine)
            if active === dropped { active = nil }
        }
        let chain = Chain(format: format)
        chain.lastUsed = useTick
        chain.nodes.forEach { engine.attach($0) }
        engine.connect(chain.player, to: chain.timePitch, format: format)
        engine.connect(chain.timePitch, to: chain.gain, format: format)
        // A mixer destination takes its next free input bus.
        engine.connect(chain.gain, to: engine.mainMixerNode, format: format)
        chains.append(chain)
        return chain
    }

    private func remove(_ chain: Chain, from engine: AVAudioEngine) {
        chain.player.stop()
        chain.nodes.forEach { engine.detach($0) }
    }

    private func makeEngine() -> AVAudioEngine {
        let engine = AVAudioEngine()
        engineSetup?(engine)
        self.engine = engine
        // Output device change: the engine stops itself; rebuild on next play.
        configObserver = NotificationCenter.default.addObserver(
            forName: .AVAudioEngineConfigurationChange, object: engine, queue: .main
        ) { [weak self] _ in
            MainActor.assumeIsolated { self?.configurationChanged() }
        }
        return engine
    }

    /// Only flags the graph: the nodes are removed on the next play, not
    /// from inside the engine's own notification.
    private func configurationChanged() {
        playId += 1
        active?.player.stop()
        graphIsStale = true
        onFinished?()
    }

    func play(_ clip: PreviewClip, loop: Bool, gainDb: Double?, pitchCents: Float) throws {
        try play(clip, loop: loop, gainDb: gainDb, pitchCents: pitchCents, startAt: 0)
    }

    func play(
        _ clip: PreviewClip, loop: Bool, gainDb: Double?, pitchCents: Float, startAt: TimeInterval
    ) throws {
        let chain = chain(for: clip.buffer.format)
        guard let engine else { return }
        // Silence the previous sound, which may be on another chain (only
        // the active chain's player ever plays).
        active?.player.stop()
        active = chain
        let player = chain.player
        playId += 1
        let id = playId
        setPitch(cents: pitchCents)
        chain.gain.globalGain = Float(min(max(gainDb ?? 0, -24), 24))
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
        guard let player = active?.player, player.isPlaying, let node = player.lastRenderTime,
            let t = player.playerTime(forNodeTime: node), t.sampleRate > 0
        else { return nil }
        return WaveformMath.playbackTime(
            elapsed: Double(t.sampleTime) / t.sampleRate, start: startOffset,
            duration: playingDuration, loop: playingLoop)
    }

    func stop() {
        playId += 1
        active?.player.stop()
    }

    /// Sets the pitch shift of the sound playing now; at 0 the time-pitch
    /// unit is bypassed (no added latency). Every `play` sets it again.
    func setPitch(cents: Float) {
        active?.timePitch.pitch = cents
        active?.timePitch.bypass = cents == 0
    }

    /// Builds the graph and starts the output once (cold start costs ~150 ms),
    /// so the first preview after launch is as fast as the rest. Best effort.
    func warmUp() {
        guard let format = AVAudioFormat(standardFormatWithSampleRate: 44100, channels: 2) else { return }
        _ = chain(for: format)
        try? engine?.start()
    }
}
