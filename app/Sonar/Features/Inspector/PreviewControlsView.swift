import SwiftUI

/// Compact preview controls: play/stop, loop, pitch (+reset), auto-play.
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
                Slider(
                    value: Binding(
                        get: { Double(preview.pitchSemitones) },
                        set: { preview.setPitch(semitones: Int($0.rounded())) }),
                    in: Double(AudioPreview.pitchRange.lowerBound)...Double(AudioPreview.pitchRange.upperBound),
                    step: 1
                )
                .accessibilityLabel("Pitch in semitones")
                Text(String(format: "%+d st", preview.pitchSemitones))
                    .monospacedDigit().frame(width: 48, alignment: .trailing)
                Button("Reset") { preview.resetPitch() }
                    .disabled(preview.pitchSemitones == 0)
            }
            Toggle("Auto-play on selection", isOn: $preview.autoPlay)
            if let status = preview.status {
                Text(status).font(.caption).foregroundStyle(.secondary)
            }
        }
    }
}
