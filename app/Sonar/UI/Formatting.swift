import Foundation

/// Display-only formatting of core values (no product logic).
enum Format {
    /// "m:ss.mmm" for a duration in milliseconds, "-" when unknown.
    static func duration(ms: UInt64?) -> String {
        guard let ms else { return "-" }
        let seconds = ms / 1000
        return String(format: "%llu:%02llu.%03llu", seconds / 60, seconds % 60, ms % 1000)
    }

    /// "128" or "127.5"; "-" when unknown.
    static func bpm(_ bpm: Double?) -> String {
        guard let bpm else { return "-" }
        return bpm == bpm.rounded() ? String(Int(bpm)) : String(format: "%.1f", bpm)
    }

    /// "1.2 MB" for a byte count.
    static func bytes(_ n: UInt64) -> String {
        ByteCountFormatter.string(fromByteCount: Int64(clamping: n), countStyle: .file)
    }

    /// Value or "-".
    static func text(_ s: String?) -> String {
        guard let s, !s.isEmpty else { return "-" }
        return s
    }
}
