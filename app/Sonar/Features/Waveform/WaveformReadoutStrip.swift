import SwiftUI

/// Readout strip at the bottom of the waveform well (DESIGN §4.4): BPM, Key,
/// Length and Format readouts, then the horizontal and vertical zoom menus.
/// Values are en dashes without a selection.
@MainActor
struct WaveformReadoutStrip: View {
    let row: FileRow?
    let model: WaveformModel

    private var isReady: Bool {
        if case .ready = model.state { return true }
        return false
    }

    var body: some View {
        HStack(spacing: Theme.Space.s16) {
            Readout("BPM", value: row?.bpm.map(Format.bpm), reserving: ["000.0"])
            divider
            Readout("Key", key: row.flatMap { KeyLabel.parts($0) })
            divider
            Readout("Length", value: row?.durationMs.map { Format.duration(ms: $0) }, reserving: ["00:00.000"])
            divider
            Readout(
                "Format", value: row.flatMap { Format.audioFormat(path: $0.path, sampleRate: $0.sampleRate, channels: $0.channels) },
                emphasised: false)
            Spacer(minLength: Theme.Space.s8)
            Text("Zoom").font(Theme.Font.row).foregroundStyle(.secondary)
            HStack(spacing: Theme.Space.s4) {
                zoomMenu(symbol: "arrow.up.and.down", label: "Vertical zoom") {
                    Button("Zoom In Vertically") { model.zoomAmplitude(by: 2) }
                        .disabled(model.amplitudeZoom >= WaveformModel.maxAmplitudeZoom)
                    Button("Zoom Out Vertically") { model.zoomAmplitude(by: 0.5) }
                        .disabled(model.amplitudeZoom <= 1)
                    Button("Reset Vertical Zoom") { model.resetAmplitudeZoom() }
                        .disabled(model.amplitudeZoom == 1)
                }
                zoomMenu(symbol: "arrow.left.and.right", label: "Horizontal zoom") {
                    Button("Zoom In") { model.zoom(by: 1.5) }
                        .disabled(model.viewport.zoom >= WaveformViewport.maxZoom)
                    Button("Zoom Out") { model.zoom(by: 1 / 1.5) }
                        .disabled(!model.viewport.isZoomed)
                    Button("Show Whole File") { model.resetZoom() }
                        .disabled(!model.viewport.isZoomed)
                }
            }
        }
        .padding(.horizontal, Theme.Space.s12)
        .frame(height: Theme.Size.readoutStripHeight)
        .background(Theme.Color.waveStrip)
    }

    private var divider: some View {
        Hairline(axis: .vertical).frame(height: Theme.Space.s16)
    }

    private func zoomMenu<Items: View>(
        symbol: String, label: String, @ViewBuilder items: () -> Items
    ) -> some View {
        Menu {
            items()
        } label: {
            Image(systemName: symbol)
                .imageScale(.small)
                .foregroundStyle(Theme.Color.wave)
                .frame(width: Theme.Size.zoomButtonWidth, height: Theme.Size.zoomButtonHeight)
                .background(Theme.Color.waveWell, in: RoundedRectangle(cornerRadius: Theme.Radius.medium))
        }
        .menuStyle(.borderlessButton)
        .menuIndicator(.hidden)
        .fixedSize()
        .disabled(!isReady)
        .help(label)
        .accessibilityLabel(label)
    }
}
