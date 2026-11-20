import SwiftUI

/// Chip (DESIGN §4.2, ADR-043): 24 pt high (22 in lists), `medium` radius,
/// `chip` font. Every chip has a facet hue.
///
/// - unselected: hue text on `chipBase` (dark) / the hue's fill at 11 % (light)
/// - hover: dark = label at 6 % over `chipBase`; light = fill at 18 %
/// - selected (`isActive`): the hue's deep fill, white text, same weight
/// - mixed (`mixed` caption such as "2/3"): transparent, dashed hue border
/// - disabled: 38 % opacity, no hover
/// Optional count caption and trailing `×` (`onClear`, own action) or symbol.
struct SonarChip: View {
    let title: String
    var count: Int?
    var isActive = false
    /// Facet hue (required, DESIGN §2.1).
    let hue: Theme.Hue
    /// "n/m" caption of a tag on only some of the selected files.
    var mixed: String?
    /// Compact (22 pt) variant for menus and lists.
    var compact = false
    /// Optional trailing SF Symbol (e.g. `xmark` on a removable tag).
    var trailingSymbol: String?
    /// Shows a separate trailing `×` button with its own action.
    var onClear: (() -> Void)?
    var clearLabel = "Clear"
    let action: () -> Void

    @State private var hovering = false
    @Environment(\.isEnabled) private var isEnabled
    @Environment(\.colorScheme) private var scheme

    private var height: CGFloat { compact ? Theme.Size.chipHeightCompact : Theme.Size.chipHeight }
    private var shape: RoundedRectangle { RoundedRectangle(cornerRadius: Theme.Radius.medium) }

    var body: some View {
        HStack(spacing: 0) {
            Button(action: action) {
                HStack(spacing: Theme.Space.s6) {
                    Text(title).font(Theme.Font.chip).lineLimit(1)
                    if let mixed {
                        Text(mixed).font(Theme.Font.captionNumeric).opacity(Theme.chipCountOpacity)
                    } else if let count {
                        Text("\(count)").font(Theme.Font.captionNumeric).opacity(Theme.chipCountOpacity)
                    }
                    if let trailingSymbol, onClear == nil {
                        Image(systemName: trailingSymbol).font(Theme.Font.caption).imageScale(.small)
                            .opacity(isActive ? Theme.chipClearOpacity : 1)
                    }
                }
                .padding(.leading, Theme.Space.s8)
                .padding(.trailing, onClear == nil ? Theme.Space.s8 : Theme.Space.s4)
                .frame(height: height)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            if let onClear {
                Button(action: onClear) {
                    Image(systemName: "xmark").font(Theme.Font.caption).imageScale(.small)
                        .opacity(isActive ? Theme.chipClearOpacity : 1)
                        .padding(.leading, Theme.Space.s2)
                        .padding(.trailing, Theme.Space.s8)
                        .frame(height: height)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel(clearLabel)
                .help(clearLabel)
            }
        }
        .foregroundStyle(isActive ? Color.white : hue.color)
        .frame(height: height)
        .background { background.clipShape(shape) }
        .overlay {
            if mixed != nil {
                shape.strokeBorder(hue.color, style: StrokeStyle(lineWidth: 1, dash: [Theme.Space.s4, Theme.Space.s2]))
            }
        }
        .contentShape(shape)
        .onHover { hovering = $0 && isEnabled }
        .opacity(isEnabled ? 1 : Theme.chipDisabledOpacity)
        .accessibilityAddTraits(isActive ? .isSelected : [])
    }

    @ViewBuilder private var background: some View {
        if isActive {
            hue.fill
        } else if mixed != nil {
            Color.clear
        } else if scheme == .dark {
            ZStack {
                Theme.Color.chipBase
                if hovering { Color.primary.opacity(Theme.chipHoverOverlayOpacity) }
            }
        } else {
            hovering ? hue.tintHover : hue.tint
        }
    }
}
