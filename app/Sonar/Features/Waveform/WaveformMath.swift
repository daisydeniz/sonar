import CoreGraphics
import Foundation

/// Pure display math for the waveform (no DSP: only min/max re-bucketing of
/// the core's peaks, viewport <-> time mapping, playhead position).

/// Horizontal viewport over the whole sound, in fractions of its duration.
struct WaveformViewport: Equatable, Sendable {
    static let maxZoom = 64.0
    static let minZoom = 1.0

    /// 1 = whole sound visible.
    private(set) var zoom = 1.0
    /// Left edge of the visible range, 0...(1 - 1/zoom).
    private(set) var start = 0.0

    init() {}

    init(zoom: Double, start: Double) {
        self.zoom = Self.clampZoom(zoom)
        self.start = min(max(start, 0), max(0, 1 - 1 / self.zoom))
    }

    /// Visible fraction of the sound.
    var span: Double { 1 / zoom }
    var end: Double { start + span }
    var isZoomed: Bool { zoom > Self.minZoom }

    static func clampZoom(_ z: Double) -> Double {
        z.isFinite ? min(max(z, minZoom), maxZoom) : minZoom
    }

    /// Fraction of the sound under `x` of `width` points.
    func fraction(atX x: Double, width: Double) -> Double {
        guard width > 0 else { return start }
        return start + min(max(x / width, 0), 1) * span
    }

    /// X in points of a sound fraction (may be outside 0...width).
    func x(forFraction f: Double, width: Double) -> Double {
        (f - start) / span * width
    }

    /// Zooms by `factor`, keeping the fraction under `anchor` (0...1 of the
    /// view width) fixed.
    func zoomed(by factor: Double, anchor: Double) -> WaveformViewport {
        let a = min(max(anchor, 0), 1)
        let pivot = start + a * span
        let newZoom = Self.clampZoom(zoom * factor)
        return WaveformViewport(zoom: newZoom, start: pivot - a / newZoom)
    }

    /// Scrolls by `deltaFraction` of the sound (positive = later).
    func panned(by deltaFraction: Double) -> WaveformViewport {
        WaveformViewport(zoom: zoom, start: start + deltaFraction)
    }

    /// Pans by a point delta of a view `width` points wide.
    func panned(byPoints dx: Double, width: Double) -> WaveformViewport {
        guard width > 0 else { return self }
        return panned(by: dx / width * span)
    }
}

enum WaveformMath {
    /// Merges `peaks` over the sound fraction range `from..<to` into `count`
    /// min/max columns. Empty columns (range narrower than a bucket) repeat
    /// the containing bucket so the shape never has holes.
    static func rebucket(
        _ peaks: [WaveformPeak], from: Double, to: Double, into count: Int
    ) -> [WaveformPeak] {
        guard !peaks.isEmpty, count > 0, to > from else { return [] }
        let n = Double(peaks.count)
        let lo = min(max(from, 0), 1) * n
        let hi = min(max(to, 0), 1) * n
        let step = (hi - lo) / Double(count)
        guard step > 0 else { return [] }
        var out: [WaveformPeak] = []
        out.reserveCapacity(count)
        for c in 0..<count {
            let a = lo + Double(c) * step
            let b = a + step
            var first = Int(a.rounded(.down))
            var last = Int(b.rounded(.up)) - 1
            first = min(max(first, 0), peaks.count - 1)
            last = min(max(last, first), peaks.count - 1)
            var mn = peaks[first].min
            var mx = peaks[first].max
            if last > first {
                for i in (first + 1)...last {
                    mn = Swift.min(mn, peaks[i].min)
                    mx = Swift.max(mx, peaks[i].max)
                }
            }
            out.append(WaveformPeak(min: mn, max: mx))
        }
        return out
    }

    /// One vertical segment per column, centred on `size.height / 2`; peaks
    /// span -1...1 over the full height. Columns are `size.width / count`
    /// wide; stroke the path with that width. Every column is at least one
    /// pixel tall so silence stays visible as a line.
    static func path(columns: [WaveformPeak], size: CGSize, minHeight: CGFloat = 1) -> CGPath {
        let path = CGMutablePath()
        guard !columns.isEmpty, size.width > 0, size.height > 0 else { return path }
        let w = size.width / CGFloat(columns.count)
        let mid = size.height / 2
        for (i, p) in columns.enumerated() {
            let x = (CGFloat(i) + 0.5) * w
            var top = mid - CGFloat(min(max(p.max, -1), 1)) * mid
            var bottom = mid - CGFloat(min(max(p.min, -1), 1)) * mid
            if bottom - top < minHeight {
                let c = (top + bottom) / 2
                top = c - minHeight / 2
                bottom = c + minHeight / 2
            }
            path.move(to: CGPoint(x: x, y: top))
            path.addLine(to: CGPoint(x: x, y: bottom))
        }
        return path
    }

    /// Time into the sound for a player that started at `start` seconds and
    /// has run `elapsed` seconds. With `loop` the first pass plays
    /// `start..<duration`, later passes the whole sound. Without it the time
    /// is clamped to `duration`.
    static func playbackTime(elapsed: Double, start: Double, duration: Double, loop: Bool) -> Double {
        guard duration > 0, elapsed.isFinite, elapsed >= 0 else { return 0 }
        let s = min(max(start, 0), duration)
        let t = s + elapsed
        if t < duration { return t }
        guard loop else { return duration }
        return (t - duration).truncatingRemainder(dividingBy: duration)
    }
}
