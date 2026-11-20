import SwiftUI

/// Height rules of the player header's waveform well (DESIGN §2.3).
enum PlayerHeaderLayout {
    static let heightKey = "player.waveformHeight"

    /// Clamps a user-chosen lanes height to 96...320 pt (ruler and readout strip come on top).
    static func clamped(_ height: Double) -> Double {
        guard height.isFinite else { return Theme.Size.waveformHeight }
        return min(max(height, Theme.Size.waveformMinHeight), Theme.Size.waveformMaxHeight)
    }
}

/// Player header (DESIGN §3, ADR-040/043): transport row (play, stop, loop,
/// auto-play, name, pitch and gain knobs) above a resizable waveform well
/// (ruler, lanes, readout strip).
/// Always present at the top of the content column; without a selection the
/// controls are disabled and the well shows a flat centre line.
@MainActor
struct PlayerHeaderView: View {
    let row: FileRow?
    @Bindable var preview: AudioPreview
    let waveform: WaveformModel
    /// Compare mode (similarity mode only): the target well above the
    /// selected waveform; nil = normal header.
    var target: TargetWell?
    var targetWaveform: WaveformModel = WaveformModel()
    var onToggleTarget: () -> Void = {}
    /// Find Similar to Selection (waveform region menu).
    var onFindSimilarToSelection: () -> Void = {}

    @AppStorage(PlayerHeaderLayout.heightKey) private var storedHeight = Theme.Size.waveformHeight
    @State private var dragStartHeight: Double?

    private var height: Double { PlayerHeaderLayout.clamped(storedHeight) }

    var body: some View {
        VStack(spacing: 0) {
            transportRow
            if let target {
                TargetWellView(
                    well: target, waveform: targetWaveform, isPlaying: preview.isTargetPlaying,
                    onTogglePlay: onToggleTarget)
                CompareCaption(prefix: "Selected", name: row?.name ?? "none") {}
            }
            WaveformView(
                model: waveform, preview: preview, row: row, onFindSimilarToSelection: onFindSimilarToSelection
            )
            .frame(height: height + Theme.Size.rulerHeight + Theme.Size.readoutStripHeight)
            .padding(.horizontal, Theme.Space.s12)
            .padding(.bottom, Theme.Space.s8)
            .overlay(alignment: .topLeading) { statusLine }
            splitHandle
        }
        .background(Theme.Color.surfaceRaised)
    }

    // MARK: Transport row

    private var transportRow: some View {
        HStack(spacing: Theme.Space.s8) {
            Button { preview.toggle() } label: {
                Image(systemName: preview.isPlaying || preview.isLoading ? "pause.fill" : "play.fill")
            }
            .buttonStyle(PlayCircleButtonStyle())
            .help("Play / Pause (Space)")
            .accessibilityLabel(preview.isPlaying ? "Pause" : "Play")
            Button { preview.stop() } label: { Image(systemName: "stop.fill") }
                .buttonStyle(TransportButtonStyle())
                .help("Stop")
                .accessibilityLabel("Stop")
            Toggle(isOn: $preview.loop) { Image(systemName: "repeat") }
                .toggleStyle(TransportToggleStyle())
                .help("Loop")
                .accessibilityLabel("Loop")
            Toggle(isOn: $preview.autoPlay) { Text("Auto").sonarLabelStyle() }
                .toggleStyle(TransportToggleStyle())
                .help("Play automatically when a sample is selected")
                .accessibilityLabel("Auto-play on selection")
            Hairline(axis: .vertical).frame(height: Theme.Space.s24)
            nameLabel
            Spacer(minLength: Theme.Space.s8)
            pitch
            gain
        }
        .controlSize(.small)
        .disabled(row == nil)
        .padding(.horizontal, Theme.Space.s12)
        .frame(height: Theme.Size.transportRowHeight)
    }

    private var nameLabel: some View {
        HStack(alignment: .firstTextBaseline, spacing: Theme.Space.s4) {
            Text(row?.name ?? "No sample selected")
                .font(Theme.Font.title)
                .foregroundStyle(row == nil ? .tertiary : .primary)
                .lineLimit(1)
                .truncationMode(.middle)
            if let row, case let ext = Format.fileExtension(path: row.path), !ext.isEmpty {
                Text("." + ext).font(Theme.Font.caption).foregroundStyle(.tertiary).lineLimit(1).fixedSize()
            }
        }
        .padding(.leading, Theme.Space.s4)
    }

    private var pitch: some View {
        knobGroup(caption: "Pitch", value: String(format: "%+d st", preview.pitchSemitones), atDefault: preview.pitchSemitones == 0) {
            Knob(
                value: Binding(get: { preview.pitchSemitones }, set: { preview.setPitch(semitones: $0) }),
                in: AudioPreview.pitchRange, defaultValue: 0, accessibilityLabel: "Pitch in semitones",
                family: .pitch)
        }
    }

    /// Preview gain (DESIGN §3): knob in tenths of a dB, 0.5 dB steps, Shift 0.1.
    private var gain: some View {
        knobGroup(caption: "Gain", value: PreviewGain.label(db: preview.gainDb), atDefault: preview.gainDb == 0) {
            Knob(
                value: Binding(
                    get: { Int((preview.gainDb * 10).rounded()) }, set: { preview.setGain(db: Double($0) / 10) }),
                in: PreviewGain.rangeTenths, defaultValue: 0, accessibilityLabel: "Preview gain in decibels",
                family: .gain, step: PreviewGain.stepTenths, fineStep: PreviewGain.fineStepTenths,
                spokenValue: PreviewGain.label(db: preview.gainDb))
        }
    }

    private func knobGroup<K: View>(
        caption: String, value: String, atDefault: Bool, @ViewBuilder knob: () -> K
    ) -> some View {
        HStack(spacing: Theme.Space.s6) {
            knob()
            VStack(alignment: .leading, spacing: Theme.Space.s2) {
                Text(caption).sonarLabelStyle().foregroundStyle(.secondary)
                Text(value).font(Theme.Font.readoutValue).foregroundStyle(atDefault ? .secondary : .primary)
            }
            .fixedSize()
        }
    }

    @ViewBuilder private var statusLine: some View {
        if let status = preview.status {
            Text(status).font(Theme.Font.caption).foregroundStyle(.secondary)
                .padding(Theme.Space.s8)
                .padding(.top, Theme.Size.rulerHeight)
        }
    }

    // MARK: Split divider

    /// Hairline with a taller drag area: drag down = taller waveform.
    private var splitHandle: some View {
        Hairline()
            .frame(maxHeight: .infinity, alignment: .bottom)
            .frame(height: Theme.Size.splitHandle)
            .contentShape(Rectangle())
            .onHover { inside in
                if inside { NSCursor.resizeUpDown.push() } else { NSCursor.pop() }
            }
            .gesture(
                DragGesture(minimumDistance: 1, coordinateSpace: .global)
                    .onChanged { drag in
                        let start = dragStartHeight ?? height
                        if dragStartHeight == nil { dragStartHeight = start }
                        storedHeight = PlayerHeaderLayout.clamped(start + drag.translation.height)
                    }
                    .onEnded { _ in dragStartHeight = nil }
            )
            .accessibilityLabel("Waveform height")
            .accessibilityAdjustableAction { direction in
                let step = Theme.Space.s24
                switch direction {
                case .increment: storedHeight = PlayerHeaderLayout.clamped(height + step)
                case .decrement: storedHeight = PlayerHeaderLayout.clamped(height - step)
                @unknown default: break
                }
            }
    }
}
