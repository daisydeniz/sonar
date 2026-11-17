import SwiftUI

/// Two-column label/value row of the inspector (DESIGN §4.9): secondary
/// `row` label on the left, value or editor on the right, at least 24 pt
/// high. Plain text values are selectable; pass `content` for editors.
struct PropertyRow<Content: View>: View {
    let label: String
    @ViewBuilder let content: Content

    init(_ label: String, @ViewBuilder content: () -> Content) {
        self.label = label
        self.content = content()
    }

    var body: some View {
        HStack(spacing: Theme.Space.s8) {
            Text(label)
                .font(Theme.Font.row).foregroundStyle(.secondary)
                .frame(width: Theme.Size.propertyLabelWidth, alignment: .leading)
            content
                .font(Theme.Font.row)
                .frame(maxWidth: .infinity, alignment: .trailing)
        }
        .frame(minHeight: Theme.Size.rowHeight)
        .accessibilityElement(children: .contain)
    }
}

extension PropertyRow where Content == PropertyValueText {
    /// Read-only value; nil / empty shows an en dash in tertiary.
    init(_ label: String, value: String?, numeric: Bool = false) {
        self.init(label) { PropertyValueText(value: value, numeric: numeric) }
    }
}

/// Right-aligned, selectable value text of a `PropertyRow`.
struct PropertyValueText: View {
    let value: String?
    var numeric = false

    var body: some View {
        if let value, !value.isEmpty {
            Text(value)
                .font(numeric ? Theme.Font.rowNumeric : Theme.Font.row)
                .foregroundStyle(.primary)
                .multilineTextAlignment(.trailing)
                .textSelection(.enabled)
        } else {
            Text("\u{2013}").font(Theme.Font.row).foregroundStyle(.tertiary)
        }
    }
}
