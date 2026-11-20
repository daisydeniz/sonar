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
    /// Preview gain in dB on top of the loudness gain (the player's output
    /// only, never the system volume); applies at once and to later plays.
    func setGain(db: Float)
    /// Pre-starts the output so the first play is fast (no-op by default).
    func warmUp()
    /// Like `play` but starting `startAt` seconds into the clip (seek).
    func play(_ clip: PreviewClip, loop: Bool, gainDb: Double?, pitchCents: Float, startAt: TimeInterval) throws
    /// Plays only the frames of `range` (a waveform region; it is also the
    /// loop range), starting `startAt` seconds into the clip.
    func play(
        _ clip: PreviewClip, loop: Bool, gainDb: Double?, pitchCents: Float, startAt: TimeInterval,
        range: Range<Int>?) throws
    /// Seconds into the clip being heard now (loop-wrapped); nil when idle.
    func playbackTime() -> TimeInterval?
}

extension PreviewPlayer {
    func warmUp() {}
    func setGain(db: Float) {}
    func playbackTime() -> TimeInterval? { nil }
    func play(
        _ clip: PreviewClip, loop: Bool, gainDb: Double?, pitchCents: Float, startAt: TimeInterval
    ) throws {
        try play(clip, loop: loop, gainDb: gainDb, pitchCents: pitchCents)
    }
    func play(
        _ clip: PreviewClip, loop: Bool, gainDb: Double?, pitchCents: Float, startAt: TimeInterval,
        range: Range<Int>?
    ) throws {
        try play(clip, loop: loop, gainDb: gainDb, pitchCents: pitchCents, startAt: startAt)
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
    /// Region (seconds) that loops / bounds the sound playing now.
    private var playingFrom: TimeInterval = 0
    private var playingTo: TimeInterval = 0

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
        try play(clip, loop: loop, gainDb: gainDb, pitchCents: pitchCents, startAt: startAt, range: nil)
    }

    func play(
        _ clip: PreviewClip, loop: Bool, gainDb: Double?, pitchCents: Float, startAt: TimeInterval,
        range: Range<Int>?
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
        normalizationDb = Float(gainDb ?? 0)
        applyGain()
        let rate = clip.buffer.format.sampleRate
        let total = Int(clip.buffer.frameLength)
        let lo = min(max(range?.lowerBound ?? 0, 0), max(total - 1, 0))
        let hi = min(max(range?.upperBound ?? total, lo + 1), max(total, 1))
        let startFrame = rate > 0 ? min(max(Int(startAt * rate), lo), max(hi - 1, lo)) : lo
        // The part that loops (or plays once): the region, or the whole clip.
        let body: AVAudioPCMBuffer? = range != nil ? Self.slice(of: clip.buffer, lo..<hi) : clip.buffer
        let tail = startFrame > lo ? Self.slice(of: clip.buffer, startFrame..<hi) : nil
        startOffset = rate > 0 ? Double(tail != nil ? startFrame : lo) / rate : 0
        playingLoop = loop
        playingFrom = rate > 0 ? Double(lo) / rate : 0
        playingTo = rate > 0 ? Double(hi) / rate : clip.duration
        if let tail {
            // Seek: play the tail first (then the body again when looping).
            player.scheduleBuffer(
                tail, at: nil, options: [], completionCallbackType: .dataPlayedBack
            ) { [weak self] _ in
                if !loop { Task { @MainActor in self?.completed(id) } }
            }
            if loop, let body { player.scheduleBuffer(body, at: nil, options: .loops, completionHandler: nil) }
        } else if let body {
            player.scheduleBuffer(
                body, at: nil, options: loop ? .loops : [], completionCallbackType: .dataPlayedBack
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

    /// Copy of the frames `frames` of `buffer` (seek tail, region).
    private static func slice(of buffer: AVAudioPCMBuffer, _ frames: Range<Int>) -> AVAudioPCMBuffer? {
        let count = frames.count
        guard count > 0, frames.upperBound <= Int(buffer.frameLength), let src = buffer.floatChannelData,
            let out = AVAudioPCMBuffer(pcmFormat: buffer.format, frameCapacity: AVAudioFrameCount(count)),
            let dst = out.floatChannelData
        else { return nil }
        for ch in 0..<Int(buffer.format.channelCount) {
            dst[ch].update(from: src[ch] + frames.lowerBound, count: count)
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
            from: playingFrom, to: playingTo, loop: playingLoop)
    }

    func stop() {
        playId += 1
        active?.player.stop()
    }

    /// Loudness-normalisation gain of the sound playing now and the user's
    /// preview gain; the EQ's global gain is their sum (EQ range -96...24).
    private var normalizationDb: Float = 0
    private var userGainDb: Float = 0

    func setGain(db: Float) {
        userGainDb = db
        applyGain()
    }

    private func applyGain() {
        active?.gain.globalGain = min(max(normalizationDb + userGainDb, -96), 24)
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
