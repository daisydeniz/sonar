import Foundation
import Observation

/// Sample preview: auto-play on selection, Space = play/stop, loop, pitch,
/// loudness normalization (ADR-034) and a pre-decoded cache of the selected
/// row and its neighbours (ARCHITECTURE §8: start < 10 ms when cached).
///
/// All decisions live here and are testable without audio hardware: audio
/// output is a `PreviewPlayer`, decoding a `PreviewDecoding`. Selection is
/// latest-wins: a decode that finishes for an older selection is cached but
/// never played.
@MainActor
@Observable
final class AudioPreview {
    static let autoPlayKey = "preview.autoPlay"
    static let normalizeKey = "preview.normalizeLoudness"
    static let pitchRange = -12...12
    static let gainKey = "preview.gainDb"
    /// Neighbour rows pre-decoded on each side of the selection.
    static let neighbourReach = 2

    private(set) var isPlaying = false
    private(set) var isLoading = false
    /// The compare-mode target (not the selection) is playing or loading.
    private(set) var isTargetPlaying = false
    /// Quiet status line (decode failure etc.); nil when fine.
    private(set) var status: String?
    var loop = false {
        didSet { if loop != oldValue { restartIfPlaying() } }
    }
    /// Selected waveform region of the current sound (fractions of its
    /// duration): play and loop use it instead of the whole sound.
    var region: WaveformRegion?
    /// Semitones, tempo preserved.
    private(set) var pitchSemitones = 0
    /// Preview gain in dB (`PreviewGain.rangeDb`), remembered; the preview
    /// engine only, never the system volume or the file.
    private(set) var gainDb = 0.0
    var autoPlay: Bool {
        didSet { defaults.set(autoPlay, forKey: Self.autoPlayKey) }
    }

    /// Apply the core's loudness-normalization gain (ADR-034). Default on.
    var normalizeLoudness: Bool {
        didSet { defaults.set(normalizeLoudness, forKey: Self.normalizeKey) }
    }

    @ObservationIgnored var decoder: PreviewDecoding?
    @ObservationIgnored let cache: PreviewCache
    @ObservationIgnored private let player: PreviewPlayer
    @ObservationIgnored private let defaults: UserDefaults
    @ObservationIgnored private let maxConcurrentPrefetch: Int
    @ObservationIgnored private var generation = 0
    @ObservationIgnored private var currentRow: FileRow?
    @ObservationIgnored private var currentClip: PreviewClip?
    @ObservationIgnored private var inflight: [Int64: Task<PreviewClip?, Never>] = [:]
    @ObservationIgnored private var queue: [FileRow] = []
    @ObservationIgnored private var activePrefetch = 0
    @ObservationIgnored private var playTask: Task<Void, Never>?

    init(
        player: PreviewPlayer = EnginePlayer(), decoder: PreviewDecoding? = nil,
        cache: PreviewCache = PreviewCache(), defaults: UserDefaults = .standard,
        maxConcurrentPrefetch: Int = 2
    ) {
        self.player = player
        self.decoder = decoder
        self.cache = cache
        self.defaults = defaults
        self.maxConcurrentPrefetch = maxConcurrentPrefetch
        autoPlay = defaults.object(forKey: Self.autoPlayKey) as? Bool ?? true
        normalizeLoudness = defaults.object(forKey: Self.normalizeKey) as? Bool ?? true
        gainDb = PreviewGain.clamped(defaults.double(forKey: Self.gainKey))
        player.setGain(db: Float(gainDb))
        player.onFinished = { [weak self] in
            self?.isPlaying = false
            self?.isTargetPlaying = false
        }
    }

    /// Pre-starts the audio output (call once at app start, not in tests).
    func warmUp() { player.warmUp() }

    // MARK: Selection

    /// The table selection changed. Stops the old sound, plays the new row
    /// when auto-play is on, and pre-decodes `neighbours` (nearest first).
    /// Re-selecting the playing row (a table refresh) only updates its data.
    func select(_ row: FileRow?, neighbours: [FileRow] = []) {
        if let row, row.fileId == currentRow?.fileId {
            currentRow = row
            prefetch(neighbours)
            return
        }
        generation += 1
        currentRow = row
        currentClip = nil
        status = nil
        endPlayback()
        guard let row else {
            queue.removeAll()
            return
        }
        if autoPlay { start(row) } else { prefetch([row] + neighbours); return }
        prefetch(neighbours)
    }

    /// Space: stop if playing or loading (the selection or the compare
    /// target), else play the selection.
    func toggle() {
        generation += 1
        if isPlaying || isLoading || isTargetPlaying {
            endPlayback()
        } else if let row = currentRow {
            status = nil
            start(row)
        }
    }

    /// Stops playback and drops any pending play of an in-flight decode.
    func stop() {
        generation += 1
        endPlayback()
    }

    /// Sets the pitch shift (clamped to `pitchRange`), applied immediately.
    func setPitch(semitones: Int) {
        pitchSemitones = min(max(semitones, Self.pitchRange.lowerBound), Self.pitchRange.upperBound)
        player.setPitch(cents: Float(pitchSemitones * 100))
    }

    func resetPitch() { setPitch(semitones: 0) }

    /// Sets the preview gain (clamped, 0.1 dB resolution), applied at once
    /// and remembered across launches.
    func setGain(db: Double) {
        gainDb = PreviewGain.clamped(db)
        defaults.set(gainDb, forKey: Self.gainKey)
        player.setGain(db: Float(gainDb))
    }

    func resetGain() { setGain(db: 0) }

    /// Playhead position as a fraction (0...1) of the selected sound, or nil
    /// when nothing plays. Cheap; polled by the waveform's display link.
    func playbackFraction() -> Double? {
        guard isPlaying, let clip = currentClip, clip.duration > 0,
            let t = player.playbackTime()
        else { return nil }
        return min(max(t / clip.duration, 0), 1)
    }

    /// "Play Selection": plays the region from its start (or the whole sound
    /// when there is none), restarting if it already plays.
    func playSelection() {
        guard let row = currentRow else { return }
        generation += 1
        endPlayback()
        status = nil
        start(row)
    }

    /// Click on the waveform: plays the selection from `fraction` (0...1).
    /// Needs the clip to be decoded already (playing, or pre-decoded in the cache).
    func seek(toFraction fraction: Double) {
        guard let row = currentRow, fraction.isFinite,
            let clip = currentClip ?? cache.get(row.fileId)
        else { return }
        generation += 1
        playTask?.cancel()
        playTask = nil
        isLoading = false
        begin(clip, row, startAt: min(max(fraction, 0), 1) * clip.duration)
    }

    // MARK: Compare-mode target

    /// The similarity target's own play button: plays the seed (its region if
    /// any) or stops it. Independent of the selection: the selected row's
    /// playhead and `currentRow` are untouched.
    func toggleTarget(_ row: FileRow, region: WaveformRegion?) {
        if isTargetPlaying {
            stop()
            return
        }
        generation += 1
        endPlayback()
        status = nil
        isTargetPlaying = true
        let mine = generation
        let task = clipTask(row)
        playTask = Task { [weak self] in
            let clip = await task.value
            guard let self, mine == self.generation else { return }
            guard let clip else {
                self.isTargetPlaying = false
                self.status = "Can't preview \(row.name)"
                return
            }
            let plan = region?.playback(
                totalFrames: Int(clip.buffer.frameLength), sampleRate: clip.buffer.format.sampleRate)
            do {
                try self.player.play(
                    clip, loop: self.loop, gainDb: self.normalizeLoudness ? row.previewGainDb : nil,
                    pitchCents: Float(self.pitchSemitones * 100), startAt: plan?.startAt ?? 0, range: plan?.range)
            } catch {
                self.isTargetPlaying = false
                self.status = "Audio output unavailable"
            }
        }
    }

    /// Stops the target if it plays (leaving similarity mode, new seed).
    func stopTarget() {
        if isTargetPlaying { stop() }
    }

    // MARK: Playback

    private func endPlayback() {
        playTask?.cancel()
        playTask = nil
        isLoading = false
        isPlaying = false
        isTargetPlaying = false
        player.stop()
    }

    private func start(_ row: FileRow) {
        if let clip = cache.get(row.fileId) {
            begin(clip, row)
            return
        }
        isLoading = true
        let mine = generation
        let task = clipTask(row)
        playTask = Task { [weak self] in
            let clip = await task.value
            guard let self, mine == self.generation else { return }
            self.isLoading = false
            if let clip {
                self.begin(clip, row)
            } else {
                self.status = "Can't preview \(row.name)"
            }
        }
    }

    /// Starts `clip`; with a region only its frames play (and loop), from the
    /// region start unless `startAt` seeks inside it.
    private func begin(_ clip: PreviewClip, _ row: FileRow, startAt: TimeInterval? = nil) {
        isTargetPlaying = false  // the selection replaces a playing compare target
        currentClip = clip
        let plan = region?.playback(
            totalFrames: Int(clip.buffer.frameLength), sampleRate: clip.buffer.format.sampleRate, startAt: startAt)
        do {
            try player.play(
                clip, loop: loop, gainDb: normalizeLoudness ? row.previewGainDb : nil,
                pitchCents: Float(pitchSemitones * 100), startAt: plan?.startAt ?? startAt ?? 0, range: plan?.range)
            isPlaying = true
        } catch {
            isPlaying = false
            status = "Audio output unavailable"
        }
    }

    private func restartIfPlaying() {
        guard isPlaying, let clip = currentClip, let row = currentRow else { return }
        begin(clip, row)
    }

    // MARK: Decoding + prefetch

    private func clipTask(_ row: FileRow) -> Task<PreviewClip?, Never> {
        if let existing = inflight[row.fileId] { return existing }
        guard let decoder else { return Task { nil } }
        let task = Task<PreviewClip?, Never> { [weak self] in
            do {
                let clip = try await decoder.decode(row)
                self?.decoded(row.fileId, clip)
                return clip
            } catch {
                self?.inflight[row.fileId] = nil
                return nil
            }
        }
        inflight[row.fileId] = task
        return task
    }

    private func decoded(_ id: Int64, _ clip: PreviewClip) {
        cache.put(id, clip)
        inflight[id] = nil
    }

    /// Replaces the prefetch queue (stale requests are dropped) and runs up
    /// to `maxConcurrentPrefetch` decodes at a time.
    private func prefetch(_ rows: [FileRow]) {
        queue = rows
        pump()
    }

    private func pump() {
        while activePrefetch < maxConcurrentPrefetch, !queue.isEmpty {
            let row = queue.removeFirst()
            if cache.contains(row.fileId) || inflight[row.fileId] != nil { continue }
            activePrefetch += 1
            let task = clipTask(row)
            Task { [weak self] in
                _ = await task.value
                self?.activePrefetch -= 1
                self?.pump()
            }
        }
    }

    /// Awaits pending decodes, prefetches and a pending start (tests).
    func settle() async {
        while true {
            if let t = playTask { await t.value }
            if let t = inflight.values.first { _ = await t.value; continue }
            if activePrefetch > 0 || !queue.isEmpty { await Task.yield(); continue }
            return
        }
    }
}
