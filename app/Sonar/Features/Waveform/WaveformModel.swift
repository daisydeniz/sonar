import Foundation
import Observation

/// Source of waveform peaks (the core in the app, fakes in tests).
protocol WaveformProviding: Sendable {
    /// `buckets` min/max pairs per channel for the whole file, one list per
    /// lane (mono 1, stereo 2; blocking work happens off the caller's
    /// thread). Throws for unreadable files.
    func peaks(fileId: Int64, buckets: Int) async throws -> [[WaveformPeak]]
}

/// Peaks from the Rust core (`waveformPeaks`, ADR-032): one list per channel
/// (ADR-043), drawn as one lane each.
struct CoreWaveformProvider: WaveformProviding {
    let engine: SonarEngine

    func peaks(fileId: Int64, buckets: Int) async throws -> [[WaveformPeak]] {
        let engine = engine
        return try await Task.detached(priority: .userInitiated) {
            try engine.waveformPeaks(fileId: fileId, buckets: UInt32(buckets))
        }.value
    }
}

/// Small LRU of peak sets by file id so arrowing back is instant.
@MainActor
final class WaveformCache {
    private let capacity: Int
    private var order: [Int64] = []  // least recently used first
    private var items: [Int64: [[WaveformPeak]]] = [:]

    init(capacity: Int = 32) { self.capacity = max(1, capacity) }

    var count: Int { items.count }

    func get(_ id: Int64) -> [[WaveformPeak]]? {
        guard let v = items[id] else { return nil }
        touch(id)
        return v
    }

    /// Like `get` without counting as a use (prefetch checks).
    func peek(_ id: Int64) -> [[WaveformPeak]]? { items[id] }

    func put(_ id: Int64, _ peaks: [[WaveformPeak]]) {
        items[id] = peaks
        touch(id)
        while order.count > capacity { items[order.removeFirst()] = nil }
    }

    private func touch(_ id: Int64) {
        order.removeAll { $0 == id }
        order.append(id)
    }
}

/// State of the waveform of the selected row.
enum WaveformState: Equatable {
    case empty
    case loading
    case ready([[WaveformPeak]])
    /// Unreadable / corrupt file: shown as "No waveform".
    case failed
}

/// Loads peaks for the selected file (latest selection wins, LRU cached) and
/// owns the zoom/scroll viewport. One high-resolution peak set per file is
/// fetched and re-bucketed for display, so zooming never calls the core.
@MainActor
@Observable
final class WaveformModel {
    /// Peak resolution requested from the core (core cap: 16384). At the
    /// 64x max zoom a 400 pt view still has > 1 bucket per 2 points.
    static let bucketCount = 8192

    private(set) var state: WaveformState = .empty
    private(set) var viewport = WaveformViewport()
    /// Amplitude zoom of the lanes (display only), 1...`maxAmplitudeZoom`.
    private(set) var amplitudeZoom = 1.0
    static let maxAmplitudeZoom = 8.0
    /// Selected time range (DESIGN §4.4); cleared when the selected sample changes.
    private(set) var region: WaveformRegion?
    /// Duration (s) of the selected sound; without it no region can be drawn.
    var duration: Double?
    /// Called whenever the region changes (the app wires it to the preview).
    @ObservationIgnored var onRegionChange: ((WaveformRegion?) -> Void)?

    @ObservationIgnored var provider: WaveformProviding?
    @ObservationIgnored private let cache: WaveformCache
    @ObservationIgnored private var generation = 0
    @ObservationIgnored private var task: Task<Void, Never>?
    @ObservationIgnored private var graceTask: Task<Void, Never>?
    @ObservationIgnored private var prefetchTask: Task<Void, Never>?
    /// Fetches in flight by file id, shared by `load` and `prefetch`.
    @ObservationIgnored private var inflight: [Int64: Task<[[WaveformPeak]]?, Never>] = [:]

    /// How long the previous waveform stays up while the next one loads
    /// before the well goes blank: a fast load swaps old → new directly,
    /// without an empty frame in between (no "blink" when arrowing).
    static let loadingGrace: Duration = .milliseconds(150)

    init(provider: WaveformProviding? = nil, cache: WaveformCache = WaveformCache()) {
        self.provider = provider
        self.cache = cache
    }

    /// The selection changed (nil = none). Resets zoom; a result for an
    /// older selection is cached but never shown. On a cache miss the
    /// previous waveform stays up for at most `loadingGrace`.
    func load(fileId: Int64?) {
        generation += 1
        task?.cancel()
        task = nil
        // Arrowing on while the old waveform is already stale: blank now
        // rather than restart the grace and keep it up indefinitely.
        let alreadyStale = graceTask != nil
        graceTask?.cancel()
        graceTask = nil
        viewport = WaveformViewport()
        amplitudeZoom = 1
        guard let fileId else {
            state = .empty
            return
        }
        if let cached = cache.get(fileId) {
            state = .ready(cached)
            return
        }
        guard provider != nil else {
            state = .failed
            return
        }
        let mine = generation
        if case .ready = state, !alreadyStale {
            graceTask = Task { [weak self] in
                try? await Task.sleep(for: Self.loadingGrace)
                guard let self, !Task.isCancelled, mine == self.generation else { return }
                self.state = .loading
            }
        } else {
            state = .loading
        }
        guard let fetch = fetch(fileId) else { return }
        task = Task { [weak self] in
            let result = await fetch.value
            guard let self, mine == self.generation else { return }
            self.graceTask?.cancel()
            self.graceTask = nil
            if let result { self.state = .ready(result) } else { self.state = .failed }
        }
    }

    /// Loads the peaks of `fileIds` (nearest neighbours first) into the cache
    /// one at a time, so arrowing to them is instant. A new call replaces the
    /// previous queue; a fetch already running finishes and is cached.
    func prefetch(_ fileIds: [Int64]) {
        prefetchTask?.cancel()
        let ids = fileIds.filter { cache.peek($0) == nil }
        guard !ids.isEmpty, provider != nil else {
            prefetchTask = nil
            return
        }
        let current = task
        prefetchTask = Task { [weak self] in
            await current?.value  // the selected file's peaks come first
            for id in ids {
                guard !Task.isCancelled, let self, self.cache.peek(id) == nil,
                      let fetch = self.fetch(id) else { continue }
                _ = await fetch.value
            }
        }
    }

    /// The shared fetch of `fileId` (cached on success; nil result = no
    /// usable peaks). Nil without a provider.
    private func fetch(_ fileId: Int64) -> Task<[[WaveformPeak]]?, Never>? {
        if let running = inflight[fileId] { return running }
        guard let provider else { return nil }
        let fetch = Task { [weak self] () -> [[WaveformPeak]]? in
            let result = try? await provider.peaks(fileId: fileId, buckets: Self.bucketCount)
            let usable = result.flatMap { Self.hasPeaks($0) ? $0 : nil }
            if let self {
                if let usable { self.cache.put(fileId, usable) }
                self.inflight[fileId] = nil
            }
            return usable
        }
        inflight[fileId] = fetch
        return fetch
    }

    /// At least one lane with at least one bucket.
    private static func hasPeaks(_ lanes: [[WaveformPeak]]) -> Bool {
        lanes.contains { !$0.isEmpty }
    }

    /// Waits for the pending load (tests).
    func settle() async {
        await task?.value
        await prefetchTask?.value
    }

    // MARK: Region

    /// Sets (or with nil clears) the region.
    func setRegion(_ new: WaveformRegion?) {
        guard new != region else { return }
        region = new
        onRegionChange?(new)
    }

    func clearRegion() { setRegion(nil) }

    // MARK: Viewport

    /// Zooms by `factor` keeping the point at `anchor` (0...1 of the view) fixed.
    func zoom(by factor: Double, anchor: Double = 0.5) {
        viewport = viewport.zoomed(by: factor, anchor: anchor)
    }
    /// Pans the zoomed viewport by `dx` points of a view `width` points wide.
    func pan(byPoints dx: Double, width: Double) {
        viewport = viewport.panned(byPoints: dx, width: width)
    }
    /// Shows the whole file again.
    func resetZoom() { viewport = WaveformViewport() }

    /// Scales the lanes' amplitude by `factor` (clamped; display only).
    func zoomAmplitude(by factor: Double) {
        let z = amplitudeZoom * factor
        amplitudeZoom = z.isFinite ? min(max(z, 1), Self.maxAmplitudeZoom) : 1
    }
    func resetAmplitudeZoom() { amplitudeZoom = 1 }
}
