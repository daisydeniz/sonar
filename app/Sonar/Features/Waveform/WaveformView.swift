import SwiftUI

/// Inspector waveform: layer-rendered peaks, playhead, click-to-seek and
/// zoom (pinch, Cmd-scroll, +/- buttons), scroll to pan when zoomed.
@MainActor
struct WaveformView: View {
    let model: WaveformModel
    let preview: AudioPreview

    var body: some View {
        VStack(spacing: 4) {
            ZStack {
                WaveformRepresentable(model: model, preview: preview)
                overlay
            }
            .frame(height: 96)
            .background(.quaternary.opacity(0.4), in: RoundedRectangle(cornerRadius: 6))
            .clipShape(RoundedRectangle(cornerRadius: 6))
            HStack(spacing: 6) {
                Spacer()
                Button { model.zoom(by: 1 / 1.5) } label: { Image(systemName: "minus.magnifyingglass") }
                    .disabled(!model.viewport.isZoomed)
                    .accessibilityLabel("Zoom out")
                Button { model.zoom(by: 1.5) } label: { Image(systemName: "plus.magnifyingglass") }
                    .disabled(model.viewport.zoom >= WaveformViewport.maxZoom)
                    .accessibilityLabel("Zoom in")
                Button("Reset") { model.resetZoom() }
                    .disabled(!model.viewport.isZoomed)
                    .accessibilityLabel("Reset zoom")
            }
            .buttonStyle(.borderless)
            .controlSize(.small)
        }
    }

    @ViewBuilder private var overlay: some View {
        switch model.state {
        case .loading:
            ProgressView().controlSize(.small)
        case .failed:
            Label("Can't read this file. It may be missing, moved or unreadable.", systemImage: "exclamationmark.triangle")
                .font(.caption).foregroundStyle(.secondary).multilineTextAlignment(.center).padding(6)
        case .empty, .ready:
            EmptyView()
        }
    }
}

private struct WaveformRepresentable: NSViewRepresentable {
    let model: WaveformModel
    let preview: AudioPreview

    func makeNSView(context: Context) -> WaveformNSView {
        WaveformNSView(frame: .zero)
    }

    func updateNSView(_ view: WaveformNSView, context: Context) {
        if case .ready(let peaks) = model.state { view.peaks = peaks } else { view.peaks = [] }
        view.viewport = model.viewport
        view.playbackFraction = { [weak preview] in preview?.playbackFraction() }
        view.onSeek = { [weak preview] f in preview?.seek(toFraction: f) }
        view.onZoom = { [weak model] factor, anchor in model?.zoom(by: factor, anchor: anchor) }
        view.onPan = { [weak model, weak view] dx in
            guard let view else { return }
            model?.pan(byPoints: dx, width: Double(view.bounds.width))
        }
        view.isPlaying = preview.isPlaying
    }
}
