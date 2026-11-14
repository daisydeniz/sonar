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
    /// Neighbour rows pre-decoded on each side of the selection.
    static let neighbourReach = 2

    private(set) var isPlaying = false
    private(set) var isLoading = false
    /// Quiet status line (decode failure etc.); nil when fine.
    private(set) var status: String?
    var loop = false {
        didSet { if loop != oldValue { restartIfPlaying() } }
    }
    /// Semitones, tempo preserved.
    private(set) var pitchSemitones = 0
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
        player.onFinished = { [weak self] in self?.isPlaying = false }
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

    /// Space: stop if playing or loading, else play the selection.
    func toggle() {
        generation += 1
        if isPlaying || isLoading {
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

    /// Playhead position as a fraction (0...1) of the selected sound, or nil
    /// when nothing plays. Cheap; polled by the waveform's display link.
    func playbackFraction() -> Double? {
        guard isPlaying, let clip = currentClip, clip.duration > 0,
            let t = player.playbackTime()
        else { return nil }
        return min(max(t / clip.duration, 0), 1)
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

    // MARK: Playback

    private func endPlayback() {
        playTask?.cancel()
        playTask = nil
        isLoading = false
        isPlaying = false
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

    private func begin(_ clip: PreviewClip, _ row: FileRow, startAt: TimeInterval = 0) {
        currentClip = clip
        do {
            try player.play(
                clip, loop: loop, gainDb: normalizeLoudness ? row.previewGainDb : nil, pitchCents: Float(pitchSemitones * 100),
                startAt: startAt)
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
