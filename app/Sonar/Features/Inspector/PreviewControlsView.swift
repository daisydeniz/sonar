import SwiftUI

/// Compact preview controls: play/stop, loop, pitch knob (double-click resets), auto-play.
@MainActor
struct PreviewControlsView: View {
    @Bindable var preview: AudioPreview

    var body: some View {
        Section("Preview") {
            HStack {
                Button {
                    preview.toggle()
                } label: {
                    Label(
                        preview.isPlaying || preview.isLoading ? "Stop" : "Play",
                        systemImage: preview.isPlaying || preview.isLoading ? "stop.fill" : "play.fill")
                }
                .accessibilityHint("Space toggles preview in the results table")
                Spacer()
                Toggle(isOn: $preview.loop) { Image(systemName: "repeat") }
                    .toggleStyle(.button)
                    .help("Loop")
                    .accessibilityLabel("Loop")
            }
            HStack {
                Text("Pitch")
                Spacer()
                Text(String(format: "%+d st", preview.pitchSemitones))
                    .monospacedDigit()
                    .foregroundStyle(preview.pitchSemitones == 0 ? .secondary : .primary)
                Knob(
                    value: Binding(
                        get: { preview.pitchSemitones },
                        set: { preview.setPitch(semitones: $0) }),
                    in: AudioPreview.pitchRange,
                    defaultValue: 0,
                    accessibilityLabel: "Pitch in semitones")
            }
            Toggle("Auto-play on selection", isOn: $preview.autoPlay)
            if let status = preview.status {
                Text(status).font(.caption).foregroundStyle(.secondary)
            }
        }
    }
}
