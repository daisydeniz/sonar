import SwiftUI

/// Player-header waveform well (DESIGN §4.4): ruler, one lane per channel
/// (layer-rendered peaks, played full `wave`, unplayed at 40 %), playhead,
/// click-to-seek, zoom (pinch, Cmd-scroll, zoom menus) and scroll-to-pan when
/// zoomed, then the readout strip. Drag selects a region (tint, edge
/// handles, readout, Play / Find Similar to Selection). Fills the height its
/// parent gives it; the well is a `waveWell` box.
@MainActor
struct WaveformView: View {
    let model: WaveformModel
    let preview: AudioPreview
    /// The selected sample (BPM for the ruler, readout strip values).
    let row: FileRow?
    /// Find Similar to Selection (the app model uses the region).
    var onFindSimilarToSelection: () -> Void = {}

    var body: some View {
        VStack(spacing: 0) {
            ZStack {
                WaveformRepresentable(
                    model: model, preview: preview, bpm: row?.bpm, onFindSimilar: onFindSimilarToSelection)
                overlay
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .overlay { regionBar }
            Theme.Color.waveGrid.frame(height: Theme.hairlineWidth)
            WaveformReadoutStrip(row: row, model: model)
        }
        .background(Theme.Color.waveWell)
        .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.well))
        .overlay {
            RoundedRectangle(cornerRadius: Theme.Radius.well)
                .strokeBorder(Theme.Color.waveGrid, lineWidth: Theme.hairlineWidth)
        }
    }

    /// Floating start/length readout and menu at the region's left edge
    /// (kept inside the lanes, below the ruler).
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
                .offset(
                    x: min(max(x + Theme.Space.s4, Theme.Space.s4), maxX),
                    y: Theme.Size.rulerHeight + Theme.Space.s4)
            }
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
    let bpm: Double?
    let onFindSimilar: () -> Void

    func makeNSView(context: Context) -> WaveformNSView {
        let view = WaveformNSView(frame: .zero)
        view.rulerHeight = Theme.Size.rulerHeight
        return view
    }

    func updateNSView(_ view: WaveformNSView, context: Context) {
        if case .ready(let peaks) = model.state { view.peaks = peaks } else { view.peaks = [] }
        view.viewport = model.viewport
        view.duration = model.duration
        view.bpm = bpm
        view.amplitudeZoom = model.amplitudeZoom
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
