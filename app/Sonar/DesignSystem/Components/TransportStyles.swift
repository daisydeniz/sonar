import SwiftUI

/// Button styles of the player header's transport row (DESIGN §3, ADR-043).
/// System `Button` / `Toggle` underneath: only the look is custom.

/// Play / pause: 30 pt circle in `controlAccent`, white symbol.
struct PlayCircleButtonStyle: ButtonStyle {
    @Environment(\.isEnabled) private var isEnabled

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .foregroundStyle(Color.white)
            .frame(width: Theme.Size.playButton, height: Theme.Size.playButton)
            .background(Theme.Color.controlAccent, in: Circle())
            .opacity(isEnabled ? (configuration.isPressed ? Theme.pressedOpacity : 1) : Theme.disabledOpacity)
    }
}

/// Stop: 28 pt square on `surfaceGroup`, radius `medium`.
struct TransportButtonStyle: ButtonStyle {
    @Environment(\.isEnabled) private var isEnabled

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .foregroundStyle(.primary)
            .frame(minWidth: Theme.Size.transportButton, minHeight: Theme.Size.transportButton)
            .background(Theme.Color.surfaceGroup, in: RoundedRectangle(cornerRadius: Theme.Radius.medium))
            .opacity(isEnabled ? (configuration.isPressed ? Theme.pressedOpacity : 1) : Theme.disabledOpacity)
    }
}

/// Loop / AUTO: like `TransportButtonStyle`; when on, `signal` text and icon
/// on `signalSoft` (never a fill behind white text).
struct TransportToggleStyle: ToggleStyle {
    @Environment(\.isEnabled) private var isEnabled

    func makeBody(configuration: Configuration) -> some View {
        Button { configuration.isOn.toggle() } label: {
            configuration.label
                .foregroundStyle(configuration.isOn ? Theme.Color.signal : Color.secondary)
                .padding(.horizontal, Theme.Space.s4)
                .frame(minWidth: Theme.Size.transportButton, minHeight: Theme.Size.transportButton)
                .background(
                    configuration.isOn ? Theme.Color.signalSoft : Theme.Color.surfaceGroup,
                    in: RoundedRectangle(cornerRadius: Theme.Radius.medium))
        }
        .buttonStyle(.plain)
        .opacity(isEnabled ? 1 : Theme.disabledOpacity)
        .accessibilityAddTraits(configuration.isOn ? .isSelected : [])
    }
}
