import SwiftUI

/// A value with a label-style caption above it (DESIGN §4.3): BPM, KEY,
/// LENGTH, PITCH in the player header. A nil value shows an en dash in tertiary.
struct Readout: View {
    let caption: String
    let value: String?

    init(_ caption: String, value: String?) {
        self.caption = caption
        self.value = value
    }

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.s2) {
            Text(caption).sonarLabelStyle().foregroundStyle(.secondary)
            if let value {
                Text(value).font(Theme.Font.readoutValue).foregroundStyle(.primary)
            } else {
                Text("\u{2013}").font(Theme.Font.readoutValue).foregroundStyle(.tertiary)
            }
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(caption)
        .accessibilityValue(value ?? "none")
    }
}
