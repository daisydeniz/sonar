import SwiftUI

/// Section heading in the label style, secondary colour, 16 above / 6 below
/// (DESIGN §4.5).
struct SectionLabel: View {
    let title: String

    init(_ title: String) { self.title = title }

    var body: some View {
        Text(title)
            .sonarLabelStyle()
            .foregroundStyle(.secondary)
            .padding(.top, Theme.Space.s16)
            .padding(.bottom, Theme.Space.s6)
            .frame(maxWidth: .infinity, alignment: .leading)
            .accessibilityAddTraits(.isHeader)
    }
}
