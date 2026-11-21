import Foundation

/// Click-versus-drag decision for a press on the waveform that did not start on a
/// region edge. Pure value type so the threshold logic is testable without a view.
/// A press that never moves past `threshold` points is a click (seek); past it,
/// the press is a drag that makes a region, however short in time.
struct RegionGesture: Equatable, Sendable {
    /// Fraction of the sound under the press.
    let anchor: Double
    /// View x of the press.
    let startX: Double
    /// Points a press may move before it counts as a drag.
    let threshold: Double
    private(set) var moved = false
    private(set) var madeRegion = false

    init(anchor: Double, startX: Double, threshold: Double) {
        self.anchor = anchor
        self.startX = startX
        self.threshold = threshold
    }

    /// What a drag step asks the view to do.
    enum DragResult: Equatable {
        /// Still inside the click threshold: nothing to show.
        case none
        /// Past the threshold: show this region (nil when it is empty).
        case region(WaveformRegion?)
    }

    /// What releasing the mouse asks the view to do.
    enum Release: Equatable {
        /// A region was made by this drag: keep it, do not seek.
        case keep
        /// A click: seek to `fraction`, clearing the region first when `clearRegion`.
        case seek(fraction: Double, clearRegion: Bool)
    }

    mutating func dragged(toX x: Double, fraction: Double, duration: Double?) -> DragResult {
        guard moved || abs(x - startX) > threshold else { return .none }
        moved = true
        let made = duration.flatMap { WaveformRegion(from: anchor, to: fraction, duration: $0) }
        madeRegion = made != nil
        return .region(made)
    }

    /// `region` is the region currently shown (a click outside it clears it).
    func released(region: WaveformRegion?) -> Release {
        if moved && madeRegion { return .keep }
        let outside = region.map { !$0.contains(anchor) } ?? false
        return .seek(fraction: anchor, clearRegion: outside)
    }
}
