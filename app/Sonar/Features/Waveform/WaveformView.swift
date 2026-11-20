import SwiftUI

/// Player-header waveform (DESIGN §4.4): layer-rendered peaks (played full
/// accent, unplayed accent at 45 %), playhead, click-to-seek, zoom (pinch,
/// Cmd-scroll, +/- buttons) and scroll-to-pan when zoomed. Drag selects a
/// region (tint, edge handles, readout, Play / Find Similar to Selection,
/// DESIGN §4.4). Fills the height its parent gives it; sits in a `waveWell`.
@MainActor
struct WaveformView: View {
    let model: WaveformModel
    let preview: AudioPreview
    /// Find Similar to Selection (the app model uses the region).
    var onFindSimilarToSelection: () -> Void = {}

    var body: some View {
        ZStack {
            WaveformRepresentable(model: model, preview: preview, onFindSimilar: onFindSimilarToSelection)
            overlay
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Theme.Color.waveWell, in: RoundedRectangle(cornerRadius: Theme.Radius.well))
        .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.well))
        .overlay(alignment: .bottomTrailing) { zoomControls }
        .overlay { regionBar }
    }

    /// Floating start/length readout and menu at the region's left edge
    /// (kept inside the well).
    @ViewBuilder private var regionBar: some View {
        if case .ready = model.state, let region = model.region, let duration = model.duration {
            GeometryReader { geo in
                let x = model.viewport.x(forFraction: region.start, width: geo.size.width)
                let maxX = max(geo.size.width - Theme.Size.regionBarWidth - Theme.Space.s4, Theme.Space.s4)
                HStack(spacing: Theme.Space.s6) {
                    Text(region.readout(duration: duration))
                        .font(Theme.Font.caption.monospacedDigit())
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                        .frame(maxWidth: .infinity, alignment: .leading)
                    Menu {
                        Button("Play Selection") { preview.playSelection() }
                        Button("Find Similar to Selection") { onFindSimilarToSelection() }
                        Divider()
                        Button("Clear Selection") { model.clearRegion() }
                    } label: {
                        Image(systemName: "ellipsis")
                    }
                    .menuStyle(.borderlessButton)
                    .menuIndicator(.hidden)
                    .controlSize(.small)
                    .fixedSize()
                    .help("Selection actions")
                    .accessibilityLabel("Selection actions")
                }
                .padding(.horizontal, Theme.Space.s6)
                .padding(.vertical, Theme.Space.s2)
                .frame(width: Theme.Size.regionBarWidth)
                .background(Theme.Color.surfaceRaised, in: RoundedRectangle(cornerRadius: Theme.Radius.small))
                .offset(x: min(max(x + Theme.Space.s4, Theme.Space.s4), maxX), y: Theme.Space.s4)
            }
        }
    }

    @ViewBuilder private var zoomControls: some View {
        if case .ready = model.state {
            HStack(spacing: Theme.Space.s4) {
                Button { model.zoom(by: 1 / 1.5) } label: { Image(systemName: "minus.magnifyingglass") }
                    .disabled(!model.viewport.isZoomed)
                    .help("Zoom out")
                    .accessibilityLabel("Zoom out")
                Button { model.zoom(by: 1.5) } label: { Image(systemName: "plus.magnifyingglass") }
                    .disabled(model.viewport.zoom >= WaveformViewport.maxZoom)
                    .help("Zoom in")
                    .accessibilityLabel("Zoom in")
                Button { model.resetZoom() } label: { Image(systemName: "arrow.left.and.right") }
                    .disabled(!model.viewport.isZoomed)
                    .help("Show whole file")
                    .accessibilityLabel("Reset zoom")
            }
            .buttonStyle(.borderless)
            .controlSize(.small)
            .padding(Theme.Space.s6)
        }
    }

    @ViewBuilder private var overlay: some View {
        switch model.state {
        case .loading, .empty, .ready:
            EmptyView()
        case .failed:
            Text("Can't read this file. It may be missing, moved or unreadable.")
                .font(Theme.Font.caption).foregroundStyle(.secondary)
                .multilineTextAlignment(.center).padding(Theme.Space.s12)
        }
    }
}

private struct WaveformRepresentable: NSViewRepresentable {
    let model: WaveformModel
    let preview: AudioPreview
    let onFindSimilar: () -> Void

    func makeNSView(context: Context) -> WaveformNSView {
        WaveformNSView(frame: .zero)
    }

    func updateNSView(_ view: WaveformNSView, context: Context) {
        if case .ready(let peaks) = model.state { view.peaks = peaks } else { view.peaks = [] }
        view.viewport = model.viewport
        view.duration = model.duration
        view.region = model.region
        view.onRegion = { [weak model] in model?.setRegion($0) }
        view.onPlaySelection = { [weak preview] in preview?.playSelection() }
        view.onFindSimilarToSelection = onFindSimilar
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
