import SwiftUI

/// Inline label + value (DESIGN §4.3) in the waveform readout strip: BPM,
/// Key, Length, Format. The caption is `row` secondary, the value
/// `readoutValue` in `signal` (Format stays primary), 6 pt apart. A nil value
/// shows an en dash in tertiary. The value slot is as wide as its widest
/// possible value, so the readouts after it do not shift while browsing.
struct Readout: View {
    let caption: String
    private let valueText: Text
    private let spoken: String?
    /// Hidden texts that size the value slot (widest values it can show).
    private let reserve: [Text]

    /// `reserving`: the widest values this readout can show (e.g. "000.0").
    init(_ caption: String, value: String?, emphasised: Bool = true, reserving: [String] = []) {
        self.caption = caption
        spoken = value
        reserve = reserving.map { Text($0).font(Theme.Font.readoutValue) }
        let color = emphasised ? Theme.Color.signal : Color.primary
        valueText = value.map { Text($0).font(Theme.Font.readoutValue).foregroundStyle(color) }
            ?? Text(KeyLabel.dash).font(Theme.Font.readoutValue).foregroundStyle(.tertiary)
    }

    /// Key / note readout: note with superscript cents, or key (see `KeyLabel`).
    init(_ caption: String, key parts: KeyLabel.Parts?) {
        self.caption = caption
        spoken = parts.map { KeyLabel.plain($0) }
        valueText = KeyLabel.text(parts, font: Theme.Font.readoutValue, color: Theme.Color.signal)
        reserve = KeyLabel.widestSamples.map { KeyLabel.text($0, font: Theme.Font.readoutValue) }
    }

    var body: some View {
        HStack(spacing: Theme.Space.s6) {
            Text(caption).font(Theme.Font.row).foregroundStyle(.secondary)
            ZStack(alignment: .leading) {
                ForEach(reserve.indices, id: \.self) { reserve[$0].hidden() }
                valueText
            }
            .lineLimit(1)
        }
        .fixedSize()
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(caption)
        .accessibilityValue(spoken ?? "none")
    }
}
