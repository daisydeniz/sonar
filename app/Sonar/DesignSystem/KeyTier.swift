import AppKit
import SwiftUI

extension Theme {
    /// Certainty class of a key (ADR-041, DESIGN §4.9): shown only as the
    /// inspector's confidence word and used by the filter. The bounds
    /// come from the core (`keyTierHigh()` / `keyTierMedium()`), never from
    /// literals here.
    enum KeyTier: Equatable, Sendable {
        case high, medium, low

        /// Core bounds, read once.
        private static let bounds = (high: keyTierHigh(), medium: keyTierMedium())

        /// Tier of a stored confidence (`nil`, 0 and NaN are low). Both bounds
        /// are inclusive lower bounds, as in the core.
        static func of(
            confidence: Float?, high: Float = bounds.high, medium: Float = bounds.medium
        ) -> KeyTier {
            guard let c = confidence, c.isFinite else { return .low }
            if c >= high { return .high }
            return c >= medium ? .medium : .low
        }

        /// Tier of a result row's key.
        static func of(_ row: FileRow) -> KeyTier { of(confidence: row.keyConf) }

        /// Confidence word of the inspector.
        var word: String {
            switch self {
            case .high: "High"
            case .medium: "Medium"
            case .low: "Low"
            }
        }
    }
}
