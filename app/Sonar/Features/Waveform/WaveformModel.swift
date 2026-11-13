import Foundation
import Observation

/// Source of waveform peaks (the core in the app, fakes in tests).
protocol WaveformProviding: Sendable {
    /// `buckets` min/max pairs for the whole file (blocking work happens off
    /// the caller's thread). Throws for unreadable files.
    func peaks(fileId: Int64, buckets: Int) async throws -> [WaveformPeak]
}

/// Peaks from the Rust core (`waveformPeaks`, ADR-032).
struct CoreWaveformProvider: WaveformProviding {
    let engine: SonarEngine

    func peaks(fileId: Int64, buckets: Int) async throws -> [WaveformPeak] {
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
    private var items: [Int64: [WaveformPeak]] = [:]

    init(capacity: Int = 32) { self.capacity = max(1, capacity) }

    var count: Int { items.count }

    func get(_ id: Int64) -> [WaveformPeak]? {
        guard let v = items[id] else { return nil }
        touch(id)
        return v
    }

    func put(_ id: Int64, _ peaks: [WaveformPeak]) {
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
    case ready([WaveformPeak])
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

    @ObservationIgnored var provider: WaveformProviding?
    @ObservationIgnored private let cache: WaveformCache
    @ObservationIgnored private var generation = 0
    @ObservationIgnored private var task: Task<Void, Never>?

    init(provider: WaveformProviding? = nil, cache: WaveformCache = WaveformCache()) {
        self.provider = provider
        self.cache = cache
    }

    /// The selection changed (nil = none). Resets zoom; a result for an
    /// older selection is cached but never shown.
    func load(fileId: Int64?) {
        generation += 1
        task?.cancel()
        task = nil
        viewport = WaveformViewport()
        guard let fileId else {
            state = .empty
            return
        }
        if let cached = cache.get(fileId) {
            state = .ready(cached)
            return
        }
        guard let provider else {
            state = .failed
            return
        }
        state = .loading
        let mine = generation
        task = Task { [weak self] in
            let result: [WaveformPeak]?
            do {
                result = try await provider.peaks(fileId: fileId, buckets: Self.bucketCount)
            } catch {
                result = nil
            }
            guard let self else { return }
            if let result, !result.isEmpty { self.cache.put(fileId, result) }
            guard mine == self.generation else { return }
            if let result, !result.isEmpty { self.state = .ready(result) } else { self.state = .failed }
        }
    }

    /// Waits for the pending load (tests).
    func settle() async { await task?.value }

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
}
