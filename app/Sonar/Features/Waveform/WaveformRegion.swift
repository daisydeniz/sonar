import Foundation

/// Which edge of a region is being dragged.
enum RegionEdge: Equatable, Sendable { case start, end }

/// Frames and start time the player needs to play a region.
struct RegionPlayback: Equatable, Sendable {
    /// Frame range of the region (the loop range when looping).
    let range: Range<Int>
    /// Seconds into the clip where this pass starts (inside the range).
    let startAt: TimeInterval
}

/// A selected time range of the sound, as fractions (0...1) of its duration,
/// so it follows zoom and pan like the playhead. Pure display/transport math:
/// similarity of the region is computed by the core (ADR-040).
struct WaveformRegion: Equatable, Sendable {
    /// Shortest region (seconds): shorter drags are clicks. Matches the core's
    /// minimum for a region seed.
    static let minDuration = 0.05

    let start: Double
    let end: Double

    /// Region between two fractions (any order, clamped to 0...1); nil when
    /// shorter than `minDuration` for a sound of `duration` seconds.
    init?(from a: Double, to b: Double, duration: Double) {
        guard a.isFinite, b.isFinite, duration.isFinite, duration > 0 else { return nil }
        let lo = min(max(min(a, b), 0), 1)
        let hi = min(max(max(a, b), 0), 1)
        guard (hi - lo) * duration >= Self.minDuration else { return nil }
        start = lo
        end = hi
    }

    func contains(_ fraction: Double) -> Bool { fraction >= start && fraction <= end }

    /// Start and end in seconds for a sound of `duration` seconds.
    func seconds(duration: Double) -> (start: Double, end: Double) {
        (start * duration, min(end * duration, duration))
    }

    /// Length in seconds.
    func length(duration: Double) -> Double { (end - start) * duration }

    /// The core's region seed (seconds from the file start).
    func similarRegion(duration: Double) -> SimilarRegion {
        let s = seconds(duration: duration)
        return SimilarRegion(startS: s.start, endS: s.end)
    }

    /// This region with `edge` moved to `fraction`, keeping at least
    /// `minDuration` between the edges and staying inside 0...1.
    func resized(_ edge: RegionEdge, to fraction: Double, duration: Double) -> WaveformRegion {
        guard duration > 0 else { return self }
        let minGap = Self.minDuration / duration
        switch edge {
        case .start:
            let s = min(max(fraction, 0), end - minGap)
            return WaveformRegion(unchecked: max(s, 0), end)
        case .end:
            let e = max(min(fraction, 1), start + minGap)
            return WaveformRegion(unchecked: start, min(e, 1))
        }
    }

    /// Frames and start time to play this region of a clip. `startAt` is a
    /// seek inside the region (clamped to it); nil starts at the region start.
    func playback(totalFrames: Int, sampleRate: Double, startAt: TimeInterval? = nil) -> RegionPlayback? {
        guard totalFrames > 0, sampleRate > 0 else { return nil }
        let lo = min(max(Int((start * Double(totalFrames)).rounded()), 0), totalFrames - 1)
        let hi = min(max(Int((end * Double(totalFrames)).rounded()), lo + 1), totalFrames)
        let loS = Double(lo) / sampleRate
        let hiS = Double(hi) / sampleRate
        return RegionPlayback(range: lo..<hi, startAt: min(max(startAt ?? loS, loS), hiS))
    }

    private init(unchecked start: Double, _ end: Double) {
        self.start = start
        self.end = end
    }

    /// "0:01.250 + 0.500 s": start and length for the readout above the well.
    func readout(duration: Double) -> String {
        let s = seconds(duration: duration).start
        let ms = UInt64((s * 1000).rounded())
        return "\(Format.duration(ms: ms))  +\(String(format: "%.3f", length(duration: duration))) s"
    }
}
