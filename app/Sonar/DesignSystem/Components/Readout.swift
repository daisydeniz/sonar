import SwiftUI

/// A value with a label-style caption above it (DESIGN §4.3): BPM, KEY,
/// LENGTH, PITCH in the player header. A nil value shows an en dash in tertiary.
struct Readout: View {
    let caption: String
    private let valueText: Text
    private let spoken: String?

    init(_ caption: String, value: String?) {
        self.caption = caption
        spoken = value
        valueText = value.map { Text($0).font(Theme.Font.readoutValue).foregroundStyle(.primary) }
            ?? Text(KeyLabel.dash).font(Theme.Font.readoutValue).foregroundStyle(.tertiary)
    }

    /// Key / note readout: note with superscript cents, or key (see `KeyLabel`).
    init(_ caption: String, key parts: KeyLabel.Parts?) {
        self.caption = caption
        spoken = parts.map { KeyLabel.plain($0) }
        valueText = KeyLabel.text(parts, font: Theme.Font.readoutValue)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.s2) {
            Text(caption).sonarLabelStyle().foregroundStyle(.secondary)
            valueText
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(caption)
        .accessibilityValue(spoken ?? "none")
    }
}
