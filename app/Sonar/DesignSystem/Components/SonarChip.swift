import SwiftUI

/// Compact chip (DESIGN §4.2): 22 pt high, `small` radius. States: default
/// (hairline border, secondary text), hover (`surfaceWell`), active
/// (`accentSoft`, accent text, no border), disabled (40 % opacity). Optional
/// count, leading category dot, trailing symbol, and (active chips only) a
/// trailing clear `×` with its own action.
struct SonarChip: View {
    let title: String
    var count: Int?
    /// Colour of the leading dot, usually `Theme.categoryColor(_:)`.
    var dot: Color?
    var isActive = false
    /// Optional trailing SF Symbol (e.g. `xmark` on a removable tag).
    var trailingSymbol: String?
    /// Shows a separate clear `×` button while active (filter chips).
    var onClear: (() -> Void)?
    var clearLabel = "Clear"
    let action: () -> Void

    @State private var hovering = false
    @Environment(\.isEnabled) private var isEnabled

    var body: some View {
        let showsClear = onClear != nil && isActive
        return HStack(spacing: 0) {
            Button(action: action) {
                HStack(spacing: Theme.Space.s4) {
                    if let dot {
                        Circle().fill(dot).frame(width: Theme.Size.categoryDot, height: Theme.Size.categoryDot)
                    }
                    Text(title).font(Theme.Font.row).lineLimit(1)
                    if let count {
                        Text("\(count)").font(Theme.Font.captionNumeric).foregroundStyle(.tertiary)
                    }
                    if let trailingSymbol {
                        Image(systemName: trailingSymbol).font(Theme.Font.caption).imageScale(.small)
                    }
                }
                .padding(.leading, Theme.Space.s8)
                .padding(.trailing, showsClear ? Theme.Space.s4 : Theme.Space.s8)
                .frame(height: Theme.Size.chipHeight)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            if let onClear, isActive {
                Button(action: onClear) {
                    Image(systemName: "xmark").font(Theme.Font.caption).imageScale(.small)
                        .padding(.leading, Theme.Space.s2)
                        .padding(.trailing, Theme.Space.s8)
                        .frame(height: Theme.Size.chipHeight)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel(clearLabel)
                .help(clearLabel)
            }
        }
        .foregroundStyle(isActive ? Theme.Color.accent : Color.secondary)
        .frame(height: Theme.Size.chipHeight)
        .background(background, in: RoundedRectangle(cornerRadius: Theme.Radius.small))
        .overlay {
            if !isActive {
                RoundedRectangle(cornerRadius: Theme.Radius.small)
                    .strokeBorder(Theme.Color.hairline, lineWidth: Theme.hairlineWidth)
            }
        }
        .contentShape(RoundedRectangle(cornerRadius: Theme.Radius.small))
        .onHover { hovering = $0 && isEnabled }
        .opacity(isEnabled ? 1 : 0.4)
        .accessibilityAddTraits(isActive ? .isSelected : [])
    }

    private var background: Color {
        if isActive { return Theme.Color.accentSoft }
        return hovering ? Theme.Color.surfaceWell : .clear
    }
}
