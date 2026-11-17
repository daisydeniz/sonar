import SwiftUI

/// Overlay shown while an audio file is dragged over the results (DESIGN §3):
/// `accentSoft` fill, accent hairline border, radius `large`, one caption.
/// Flat: no blur, no shadow. Never intercepts the drop.
struct DropOverlayView: View {
    var message = "Drop an audio file to find similar sounds"
    var symbol = "waveform.badge.magnifyingglass"

    var body: some View {
        RoundedRectangle(cornerRadius: Theme.Radius.large)
            .fill(Theme.Color.surfaceContent)
            .overlay { RoundedRectangle(cornerRadius: Theme.Radius.large).fill(Theme.Color.accentSoft) }
            .overlay {
                RoundedRectangle(cornerRadius: Theme.Radius.large)
                    .strokeBorder(Theme.Color.accent, lineWidth: Theme.Space.s2)
            }
            .overlay {
                Label(message, systemImage: symbol)
                    .font(Theme.Font.title)
                    .foregroundStyle(Theme.Color.accent)
            }
            .padding(Theme.Space.s8)
            .allowsHitTesting(false)
            .accessibilityElement(children: .combine)
    }
}
