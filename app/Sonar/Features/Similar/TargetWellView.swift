import SwiftUI

/// Caption row of a compare-mode well: "Similar to: name" / "Selected: name"
/// (`caption`, middle truncation) with optional trailing content (play button).
struct CompareCaption<Trailing: View>: View {
    let prefix: String
    let name: String
    @ViewBuilder var trailing: () -> Trailing

    var body: some View {
        HStack(spacing: Theme.Space.s6) {
            trailing()
            Text("\(prefix): \(name)")
                .font(Theme.Font.caption)
                .foregroundStyle(.secondary)
                .lineLimit(1)
                .truncationMode(.middle)
            Spacer(minLength: 0)
        }
        .padding(.horizontal, Theme.Space.s12)
        .frame(height: Theme.Size.compareLabelHeight)
    }
}

/// Compare mode's similarity-target well (DESIGN §3, ADR-040): caption with
/// its own play button above a read-only 72 pt waveform. The seed region, if
/// the search used one, is drawn like a selection (tint + accent edges).
/// External seeds without peaks show a flat centre line.
@MainActor
struct TargetWellView: View {
    let well: TargetWell
    let waveform: WaveformModel
    let isPlaying: Bool
    let onTogglePlay: () -> Void

    var body: some View {
        VStack(spacing: 0) {
            CompareCaption(prefix: "Similar to", name: well.name) {
                Button(action: onTogglePlay) {
                    Image(systemName: isPlaying ? "stop.fill" : "play.fill")
                        .imageScale(.small)
                }
                .buttonStyle(.borderless)
                .controlSize(.small)
                .disabled(well.row == nil)
                .help(isPlaying ? "Stop the target" : "Play the target")
                .accessibilityLabel(isPlaying ? "Stop target" : "Play target")
            }
            TargetWaveform(waveform: waveform)
                .frame(height: Theme.Size.compareWellHeight)
                .background(Theme.Color.surfaceWell, in: RoundedRectangle(cornerRadius: Theme.Radius.medium))
                .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.medium))
                .padding(.horizontal, Theme.Space.s12)
                .accessibilityLabel("Waveform of \(well.name)")
        }
    }
}

/// Read-only `WaveformNSView`: no seek, no region editing, no zoom.
private struct TargetWaveform: NSViewRepresentable {
    let waveform: WaveformModel

    func makeNSView(context: Context) -> WaveformNSView { WaveformNSView(frame: .zero) }

    func updateNSView(_ view: WaveformNSView, context: Context) {
        if case .ready(let peaks) = waveform.state { view.peaks = peaks } else { view.peaks = [] }
        view.duration = waveform.duration
        view.region = waveform.region
    }
}
