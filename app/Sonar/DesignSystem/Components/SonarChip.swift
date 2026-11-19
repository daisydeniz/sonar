import SwiftUI

/// Compact chip (DESIGN §4.2): 22 pt high, `small` radius. States: default
/// (hairline border, secondary text), hover (`surfaceWell`), active
/// (`accentSoft`, accent text, no border), disabled (40 % opacity). Optional
/// count, leading category dot, trailing symbol, and (active chips only) a
/// trailing clear `×` with its own action.
///
/// With a `hue` (ADR-042, Logic Pro style) the chip is coloured: hue text on
/// the hue at 16 % (hover 24 %), no border; active = the hue's deep fill with
/// white text. Weight never changes, so a chip cloud does not reflow.
struct SonarChip: View {
    let title: String
    var count: Int?
    /// Colour of the leading dot, usually `Theme.categoryColor(_:)`.
    var dot: Color?
    var isActive = false
    /// Facet hue (filter panel, active filter chips, tags); nil = plain chip.
    var hue: Theme.Hue?
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
                    if let count { countText(count) }
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
        .foregroundStyle(foreground)
        .frame(height: Theme.Size.chipHeight)
        .background(background, in: RoundedRectangle(cornerRadius: Theme.Radius.small))
        .overlay {
            if !isActive && hue == nil {
                RoundedRectangle(cornerRadius: Theme.Radius.small)
                    .strokeBorder(Theme.Color.hairline, lineWidth: Theme.hairlineWidth)
            }
        }
        .contentShape(RoundedRectangle(cornerRadius: Theme.Radius.small))
        .onHover { hovering = $0 && isEnabled }
        .opacity(isEnabled ? 1 : 0.4)
        .accessibilityAddTraits(isActive ? .isSelected : [])
    }

    @ViewBuilder
    private func countText(_ count: Int) -> some View {
        let text = Text("\(count)").font(Theme.Font.captionNumeric)
        if hue == nil {
            text.foregroundStyle(.tertiary)
        } else {
            text.opacity(Theme.chipCountOpacity)
        }
    }

    private var foreground: Color {
        if let hue { return isActive ? Color.white : hue.color }
        return isActive ? Theme.Color.accent : Color.secondary
    }

    private var background: Color {
        if let hue { return isActive ? hue.fill : (hovering ? hue.tintHover : hue.tint) }
        if isActive { return Theme.Color.accentSoft }
        return hovering ? Theme.Color.surfaceWell : .clear
    }
}
