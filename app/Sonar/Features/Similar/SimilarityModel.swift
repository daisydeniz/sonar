import Foundation
import Observation

/// What a similarity query looks for neighbours of.
enum SimilarityTarget: Equatable, Sendable {
    /// A file already in the library (must be analyzed).
    case file(id: Int64, name: String)
    /// A file outside the library (decoded and analyzed on the fly by the core).
    case external(url: URL)

    var displayName: String {
        switch self {
        case .file(_, let name): return name
        case .external(let url): return url.deletingPathExtension().lastPathComponent
        }
    }

    /// The core's seed record.
    var seed: SimilarSeed {
        switch self {
        case .file(let id, _): return .file(fileId: id)
        case .external(let url): return .external(path: url.path)
        }
    }
}

/// The five similarity sliders (PRODUCT §5). Values are 0...1 and only
/// passed to the core, which maps them to aspect weights.
enum SimilarityAspect: CaseIterable, Sendable {
    case overall, spectrum, timbre, pitch, amplitude

    var title: String {
        switch self {
        case .overall: return "Overall"
        case .spectrum: return "Spectrum"
        case .timbre: return "Timbre"
        case .pitch: return "Pitch"
        case .amplitude: return "Amplitude"
        }
    }
}

/// Slider values; the default is Overall = 1, the aspects 0.
struct AspectSliders: Equatable, Sendable {
    var overall: Float = 1
    var spectrum: Float = 0
    var timbre: Float = 0
    var pitch: Float = 0
    var amplitude: Float = 0

    subscript(aspect: SimilarityAspect) -> Float {
        get {
            switch aspect {
            case .overall: return overall
            case .spectrum: return spectrum
            case .timbre: return timbre
            case .pitch: return pitch
            case .amplitude: return amplitude
            }
        }
        set {
            let v = min(max(newValue, 0), 1)
            switch aspect {
            case .overall: overall = v
            case .spectrum: spectrum = v
            case .timbre: timbre = v
            case .pitch: pitch = v
            case .amplitude: amplitude = v
            }
        }
    }

    /// Weight mapping stays in the core; throws `InvalidInput` when all zero.
    func weights() throws -> AspectWeights {
        try aspectWeightsFromSliders(
            overall: overall, spectrum: spectrum, timbre: timbre, pitch: pitch, amplitude: amplitude)
    }
}

/// Main-actor state of similarity mode: the seed, the sliders and the status
/// shown in the bar. The query itself runs in `AppModel.refreshSimilar`
/// (shares the model's generation counter, so it is latest-wins against
/// text searches too).
@MainActor
@Observable
final class SimilarityModel {
    /// The current seed; non-nil = similarity mode is on.
    private(set) var target: SimilarityTarget?
    var sliders = AspectSliders()
    /// A query is running.
    private(set) var isLoading = false
    /// Inline status of the last query (error or hint); nil when fine.
    private(set) var message: String?
    /// Distances of the shown rows, in row order.
    private(set) var hits: [SimilarHit] = []

    var isActive: Bool { target != nil }

    /// Delay before a slider change re-queries.
    @ObservationIgnored var debounceNs: UInt64 = 150_000_000
    /// Replaces the engine call (tests); the app uses `CoreBridge.similarTo`.
    @ObservationIgnored var searchOverride: (@Sendable (SimilarQuery) async throws -> SimilarPage)?
    @ObservationIgnored private(set) var debounceToken = 0

    func begin(_ newTarget: SimilarityTarget) {
        if !isActive { sliders = AspectSliders() }
        target = newTarget
        message = nil
        hits = []
        isLoading = true
    }

    func exit() {
        target = nil
        message = nil
        hits = []
        isLoading = false
    }

    func markLoading() { isLoading = true }

    func finish(hits: [SimilarHit]) {
        self.hits = hits
        message = nil
        isLoading = false
    }

    func fail(_ error: Error) {
        hits = []
        isLoading = false
        message = Self.message(for: error, target: target)
    }

    func nextDebounceToken() -> Int {
        debounceToken += 1
        return debounceToken
    }

    /// User-facing text for a failed query (no modal alerts).
    static func message(for error: Error, target: SimilarityTarget?) -> String {
        guard let e = error as? FfiError else { return error.localizedDescription }
        switch e {
        case .NotFound:
            return "This file isn't analyzed yet"
        case .Internal(let m) where m.lowercased().contains("vector"):
            return "Similarity is available once indexing has analyzed your library"
        case .InvalidInput:
            return "Raise at least one slider above zero"
        case .Audio(let m), .Io(let m):
            if case .external = target { return "Couldn't read that audio file: \(m)" }
            return m
        case .Internal(let m), .License(let m): return m
        case .Cancelled: return "Cancelled"
        }
    }
}
