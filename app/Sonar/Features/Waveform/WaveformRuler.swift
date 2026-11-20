import Foundation

/// One tick of the waveform ruler.
struct RulerTick: Equatable {
    /// Position as a fraction of the sound's duration.
    let fraction: Double
    /// Text next to the tick; nil = unlabelled.
    let label: String?
    /// Long tick (bar, beat or labelled step) vs short subdivision tick.
    let major: Bool
}

/// Ruler ticks of the waveform well (DESIGN §4.4): bars.beats with a BPM
/// (4/4, sixteenth ticks), seconds without. Pure display math over the
/// viewport; the BPM comes from the core.
enum WaveformRuler {
    /// Minimum distance (pt) between two labels.
    static let minLabelSpacing = 60.0
    /// Minimum distance (pt) between two ticks.
    static let minTickSpacing = 8.0
    private static let beatsPerBar = 4
    private static let sixteenthsPerBeat = 4
    private static let secondSteps: [Double] = [
        0.01, 0.02, 0.05, 0.1, 0.2, 0.5, 1, 2, 5, 10, 15, 30, 60, 120, 300, 600, 1800, 3600,
    ]

    /// Ticks for the visible part of a sound `duration` seconds long drawn
    /// `width` points wide. Empty for a degenerate input.
    static func ticks(
        duration: Double, bpm: Double?, viewport: WaveformViewport, width: Double
    ) -> [RulerTick] {
        guard duration > 0, duration.isFinite, width > 0 else { return [] }
        let pxPerSec = width / (duration * viewport.span)
        let from = viewport.start * duration
        let to = viewport.end * duration
        if let bpm, bpm.isFinite, bpm > 0 {
            return barTicks(bpm: bpm, from: from, to: to, duration: duration, pxPerSec: pxPerSec)
        }
        return secondTicks(from: from, to: to, duration: duration, pxPerSec: pxPerSec)
    }

    /// Smallest second step whose labels are at least `minLabelSpacing` apart.
    static func secondStep(pxPerSec: Double) -> Double {
        secondSteps.first { $0 * pxPerSec >= minLabelSpacing } ?? secondSteps[secondSteps.count - 1]
    }

    private static func secondTicks(from: Double, to: Double, duration: Double, pxPerSec: Double) -> [RulerTick] {
        let step = secondStep(pxPerSec: pxPerSec)
        // 0.02 / 2 / 20 steps split in 4, the others in 5 (0.5 -> 0.1, 1 -> 0.2).
        let leading = Int((step / pow(10, floor(log10(step)))).rounded())
        let divisions = leading == 2 ? 4 : 5
        let minor = step / Double(divisions)
        let showMinor = minor * pxPerSec >= minTickSpacing
        let unit = showMinor ? minor : step
        let perStep = showMinor ? divisions : 1
        let decimals = step < 0.1 ? 2 : (step < 1 ? 1 : 0)
        var out: [RulerTick] = []
        var i = Int((from / unit - 1e-9).rounded(.up))
        while Double(i) * unit <= to + unit * 1e-6 {
            let t = Double(i) * unit
            let major = i % perStep == 0
            let label = major ? String(format: "%.*f", decimals, t) : nil
            out.append(RulerTick(fraction: t / duration, label: label, major: major))
            i += 1
        }
        return out
    }

    private static func barTicks(
        bpm: Double, from: Double, to: Double, duration: Double, pxPerSec: Double
    ) -> [RulerTick] {
        let sixteenth = 60 / bpm / Double(sixteenthsPerBeat)
        let perBeat = sixteenthsPerBeat
        let perBar = sixteenthsPerBeat * beatsPerBar
        let pxPerBeat = sixteenth * Double(perBeat) * pxPerSec
        let pxPerBar = pxPerBeat * Double(beatsPerBar)
        // Labels on every beat when they fit, else on every 2^k-th bar.
        let beatLabels = pxPerBeat >= minLabelSpacing
        var barEvery = 1
        while !beatLabels, pxPerBar * Double(barEvery) < minLabelSpacing, barEvery < 1 << 20 { barEvery *= 2 }
        let unit: Int
        if sixteenth * pxPerSec >= minTickSpacing {
            unit = 1
        } else if pxPerBeat >= minTickSpacing {
            unit = perBeat
        } else {
            unit = perBar
        }
        var out: [RulerTick] = []
        var i = Int((from / sixteenth / Double(unit) - 1e-9).rounded(.up)) * unit
        while Double(i) * sixteenth <= to + sixteenth * 1e-6 {
            let bar = i / perBar + 1
            let beat = (i % perBar) / perBeat + 1
            var label: String?
            if i % perBar == 0 {
                label = (bar - 1) % barEvery == 0 ? "\(bar)" : nil
            } else if beatLabels, i % perBeat == 0 {
                label = "\(bar).\(beat)"
            }
            out.append(RulerTick(fraction: Double(i) * sixteenth / duration, label: label, major: i % perBeat == 0))
            i += unit
        }
        return out
    }
}
