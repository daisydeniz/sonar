import Foundation

/// The preview gain control (DESIGN §3, ADR-043): -24...+6 dB in 0.5 dB
/// steps (0.1 with Shift), applied in the preview engine only. The knob
/// works in tenths of a dB (integers); these helpers convert and format.
enum PreviewGain {
    static let rangeDb: ClosedRange<Double> = -24...6
    /// The knob's range in tenths of a dB.
    static let rangeTenths: ClosedRange<Int> = -240...60
    static let stepTenths = 5
    static let fineStepTenths = 1

    /// `db` limited to the range and rounded to 0.1 dB; non-finite = 0.
    static func clamped(_ db: Double) -> Double {
        guard db.isFinite else { return 0 }
        return (min(max(db, rangeDb.lowerBound), rangeDb.upperBound) * 10).rounded() / 10
    }

    /// Amplitude factor of a gain in dB (0 dB = 1, -6 dB ~ 0.5).
    static func linear(db: Double) -> Double { pow(10, clamped(db) / 20) }

    /// "-3.0 dB" with a true minus sign, "+1.5 dB", "0.0 dB".
    static func label(db: Double) -> String {
        let v = clamped(db)
        if v == 0 { return "0.0 dB" }
        let sign = v < 0 ? "\u{2212}" : "+"
        return sign + String(format: "%.1f dB", abs(v))
    }
}
