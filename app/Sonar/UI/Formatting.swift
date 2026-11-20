import Foundation

/// Display-only formatting of core values (no product logic).
enum Format {
    /// "m:ss.mmm" for a duration in milliseconds, "-" when unknown.
    static func duration(ms: UInt64?) -> String {
        guard let ms else { return "-" }
        let seconds = ms / 1000
        return String(format: "%llu:%02llu.%03llu", seconds / 60, seconds % 60, ms % 1000)
    }

    /// "128" or "127.5"; "-" when unknown. Rounds to one decimal first so
    /// 122.96 shows as "123", never "123.0".
    static func bpm(_ bpm: Double?) -> String {
        guard let bpm, bpm.isFinite else { return "-" }
        let tenths = (bpm * 10).rounded() / 10
        return tenths == tenths.rounded() ? String(Int(tenths)) : String(format: "%.1f", tenths)
    }

    /// File extension without the dot, lowercased ("wav"); empty when none.
    static func fileExtension(path: String) -> String {
        URL(fileURLWithPath: path).pathExtension.lowercased()
    }

    /// "WAV · 44.1 kHz · Stereo" for the waveform readout strip; parts the
    /// file lacks are left out, nil when nothing is known.
    static func audioFormat(path: String, sampleRate: UInt32?, channels: UInt16?) -> String? {
        var parts: [String] = []
        let ext = fileExtension(path: path)
        if !ext.isEmpty { parts.append(ext.uppercased()) }
        if let sampleRate, sampleRate > 0 {
            let khz = Double(sampleRate) / 1000
            let text = khz == khz.rounded() ? String(Int(khz)) : String(format: "%.1f", khz)
            parts.append(text + " kHz")
        }
        if let channels, channels > 0 {
            parts.append(channels == 1 ? "Mono" : (channels == 2 ? "Stereo" : "\(channels) ch"))
        }
        return parts.isEmpty ? nil : parts.joined(separator: " \u{00B7} ")
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

    // MARK: Table cells (missing value = empty, never "-")

    /// BPM for a table cell: empty when unknown.
    static func cellBPM(_ bpm: Double?) -> String { bpm.map { Format.bpm($0) } ?? "" }

    /// Duration for a table cell: empty when unknown.
    static func cellDuration(ms: UInt64?) -> String { ms.map { Format.duration(ms: $0) } ?? "" }

    /// Text for a table cell: empty when unknown.
    static func cellText(_ s: String?) -> String { s ?? "" }

    /// Where a key came from: "Audio", "Name", "Audio + Name", "You" (override).
    static func keySource(_ source: String?) -> String? {
        switch source {
        case "detected": "Audio"
        case "filename": "Name"
        case "both": "Audio + Name"
        case "user": "You"
        default: nil
        }
    }

    /// "Audio + Name · High" (source and confidence word); nil without key.
    static func keyDetail(source: String?, tier: Theme.KeyTier) -> String? {
        keySource(source).map { "\($0) · \(tier.word)" }
    }

    /// "kick" -> "Kick" (first letter capitalised); empty when unknown.
    static func categoryTitle(_ category: String?) -> String {
        guard let category, !category.isEmpty else { return "" }
        if category == "fx" { return "FX" }
        return category.prefix(1).uppercased() + category.dropFirst()
    }

    /// Display name of a core kind string: "loop" -> "Loop", "one_shot" ->
    /// "One-shot"; empty when unknown. Core strings are not changed.
    static func kindTitle(_ kind: String?) -> String {
        switch kind {
        case nil, "": return ""
        case "loop": return "Loop"
        case "one_shot": return "One-shot"
        case let other?: return categoryTitle(other.replacingOccurrences(of: "_", with: " "))
        }
    }

    /// SF Symbol of a core kind string, nil when unknown.
    static func kindSymbol(_ kind: String?) -> String? {
        switch kind {
        case nil, "": return nil
        case "loop": return "repeat"
        default: return "waveform"
        }
    }
}
