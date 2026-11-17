import SwiftUI

/// Small status label (DESIGN §4.10): `caption` text in a status colour on
/// the same colour at 16 %, radius `small`. Online / offline libraries,
/// exact / near duplicate groups.
struct StatusBadge: View {
    let text: String
    let color: Color

    var body: some View {
        Text(text)
            .font(Theme.Font.caption)
            .foregroundStyle(color)
            .padding(.horizontal, Theme.Space.s6)
            .padding(.vertical, Theme.Space.s2)
            .background(color.opacity(0.16), in: RoundedRectangle(cornerRadius: Theme.Radius.small))
    }
}
