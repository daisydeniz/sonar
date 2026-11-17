import SwiftUI

/// Height rules of the player header's waveform well (DESIGN §2.3).
enum PlayerHeaderLayout {
    static let heightKey = "player.waveformHeight"

    /// Clamps a user-chosen waveform height to 120...360 pt.
    static func clamped(_ height: Double) -> Double {
        guard height.isFinite else { return Theme.Size.waveformHeight }
        return min(max(height, Theme.Size.waveformMinHeight), Theme.Size.waveformMaxHeight)
    }
}

/// Player header (DESIGN §3, ADR-040): transport row (play/stop, loop,
/// auto-play, name, pitch, BPM/KEY/LENGTH) above a resizable waveform well.
/// Always present at the top of the content column; without a selection the
/// controls are disabled and the well shows a flat centre line.
@MainActor
struct PlayerHeaderView: View {
    let row: FileRow?
    @Bindable var preview: AudioPreview
    let waveform: WaveformModel

    @AppStorage(PlayerHeaderLayout.heightKey) private var storedHeight = Theme.Size.waveformHeight
    @State private var dragStartHeight: Double?

    private var height: Double { PlayerHeaderLayout.clamped(storedHeight) }

    var body: some View {
        VStack(spacing: 0) {
            transportRow
            WaveformView(model: waveform, preview: preview)
                .frame(height: height)
                .padding(.horizontal, Theme.Space.s12)
                .overlay(alignment: .bottomLeading) { statusLine }
            splitHandle
        }
        .background(Theme.Color.surfaceRaised)
    }

    // MARK: Transport row

    private var transportRow: some View {
        HStack(spacing: Theme.Space.s8) {
            Group {
                Button { preview.toggle() } label: {
                    Image(systemName: preview.isPlaying || preview.isLoading ? "pause.fill" : "play.fill")
                        .frame(width: Theme.Size.transportButton, height: Theme.Size.transportButton)
                }
                .help("Play / Pause (Space)")
                .accessibilityLabel(preview.isPlaying ? "Pause" : "Play")
                Button { preview.stop() } label: {
                    Image(systemName: "stop.fill")
                        .frame(width: Theme.Size.transportButton, height: Theme.Size.transportButton)
                }
                .help("Stop")
                .accessibilityLabel("Stop")
                Toggle(isOn: $preview.loop) {
                    Image(systemName: "repeat")
                        .frame(width: Theme.Size.transportButton, height: Theme.Size.transportButton)
                }
                .toggleStyle(.button)
                .help("Loop")
                .accessibilityLabel("Loop")
                Toggle(isOn: $preview.autoPlay) { Text("Auto").font(Theme.Font.row) }
                    .toggleStyle(.button)
                    .help("Play automatically when a sample is selected")
                    .accessibilityLabel("Auto-play on selection")
            }
            .buttonStyle(.borderless)
            .controlSize(.small)

            Text(row?.name ?? "No sample selected")
                .font(Theme.Font.title)
                .foregroundStyle(row == nil ? .tertiary : .primary)
                .lineLimit(1)
                .truncationMode(.middle)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.leading, Theme.Space.s8)

            pitch
            HStack(spacing: Theme.Space.s16) {
                Readout("BPM", value: row?.bpm.map(Format.bpm))
                Readout("KEY", value: row?.key.flatMap { $0.isEmpty ? nil : $0 })
                Readout("LENGTH", value: row?.durationMs.map { Format.duration(ms: $0) })
            }
            .padding(.leading, Theme.Space.s8)
        }
        .disabled(row == nil)
        .padding(.horizontal, Theme.Space.s12)
        .frame(height: Theme.Size.barHeight)
    }

    private var pitch: some View {
        HStack(spacing: Theme.Space.s6) {
            Knob(
                value: Binding(get: { preview.pitchSemitones }, set: { preview.setPitch(semitones: $0) }),
                in: AudioPreview.pitchRange, defaultValue: 0, accessibilityLabel: "Pitch in semitones")
            Text(String(format: "%+d st", preview.pitchSemitones))
                .font(Theme.Font.rowNumeric)
                .foregroundStyle(preview.pitchSemitones == 0 ? .secondary : .primary)
        }
    }

    @ViewBuilder private var statusLine: some View {
        if let status = preview.status {
            Text(status).font(Theme.Font.caption).foregroundStyle(.secondary)
                .padding(Theme.Space.s8)
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
