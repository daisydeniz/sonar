import SwiftUI
@testable import Sonar

/// Every v2 design token and shared component on one page (UI snapshot
/// `components`), laid out like `design/reference/sonar-v2/components-*.png`.
struct ComponentsGallery: View {
    private struct ChipRow: Identifiable {
        let id: String
        let hue: Theme.Hue
        let unselected: String
        let selected: String
        let count: Int
    }

    private let rows: [ChipRow] = [
        ChipRow(id: "Category", hue: .category, unselected: "Kick", selected: "Drums", count: 12),
        ChipRow(id: "Key", hue: .key, unselected: "C#", selected: "Am", count: 12),
        ChipRow(id: "Tempo / Duration", hue: .property, unselected: "≤ 90", selected: "120–140", count: 12),
        ChipRow(id: "Tags", hue: .tag, unselected: "dusty", selected: "warm", count: 12),
        ChipRow(id: "Library", hue: .library, unselected: "Drum Hits", selected: "KSHMR Vol 1", count: 12),
    ]

    private let swatches: [(String, Color)] = [
        ("accent", Theme.Color.accent), ("controlAccent", Theme.Color.controlAccent), ("signal", Theme.Color.signal),
        ("surfaceContent", Theme.Color.surfaceContent), ("surfaceRaised", Theme.Color.surfaceRaised),
        ("surfaceGroup", Theme.Color.surfaceGroup), ("chipBase", Theme.Color.chipBase),
        ("waveWell", Theme.Color.waveWell), ("waveStrip", Theme.Color.waveStrip), ("wave", Theme.Color.wave),
        ("selection", Theme.Color.selection), ("selectionInactive", Theme.Color.selectionInactive),
        ("signalSoft", Theme.Color.signalSoft), ("hairline", Theme.Color.hairline), ("favorite", Theme.Color.favorite),
        ("error", Theme.Color.error), ("warning", Theme.Color.warning), ("ok", Theme.Color.ok),
    ]

    var body: some View {
        HStack(alignment: .top, spacing: Theme.Space.s16) {
            VStack(alignment: .leading, spacing: Theme.Space.s16) {
                chipCard
                typeCard
            }
            VStack(alignment: .leading, spacing: Theme.Space.s16) {
                knobCard
                transportCard
            }
            swatchCard
        }
        .padding(Theme.Space.s16)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .background(Theme.Color.surfaceContent)
    }

    private func card<Content: View>(_ title: String, @ViewBuilder _ content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: Theme.Space.s8) {
            Text(title).sonarLabelStyle().foregroundStyle(.secondary)
            content()
        }
        .padding(Theme.Space.s12)
        .background(Theme.Color.surfaceGroup, in: RoundedRectangle(cornerRadius: Theme.Radius.large))
    }

    // MARK: Chips

    private var chipCard: some View {
        card("Chips — section hue") {
            Grid(alignment: .leading, horizontalSpacing: Theme.Space.s8, verticalSpacing: Theme.Space.s8) {
                GridRow {
                    Text("").gridCellUnsizedAxes([.horizontal, .vertical])
                    ForEach(["Unselected", "Selected", "Disabled", "Mixed"], id: \.self) {
                        Text($0).font(Theme.Font.caption).foregroundStyle(.tertiary)
                    }
                }
                ForEach(rows) { row in
                    GridRow {
                        Text(row.id).font(Theme.Font.row).foregroundStyle(.secondary)
                        SonarChip(title: row.unselected, count: row.count, hue: row.hue, action: {})
                        SonarChip(title: row.selected, isActive: true, hue: row.hue, onClear: {}, action: {})
                        SonarChip(title: row.unselected, count: 0, hue: row.hue, action: {}).disabled(true)
                        SonarChip(title: row.unselected, hue: row.hue, mixed: "2/3", action: {})
                    }
                }
            }
        }
    }

    private var typeCard: some View {
        card("Type scale") {
            Text("Title 15 semibold").font(Theme.Font.title)
            Text("Body 13 regular").font(Theme.Font.body)
            Text("Row 12 regular  128.0 BPM  0:03.4").font(Theme.Font.row)
            Text("Chip 12 medium").font(Theme.Font.chip)
            Text("Caption 11 regular").font(Theme.Font.caption)
            Text("Label style").sonarLabelStyle().foregroundStyle(.secondary)
            Text("Readout value 138").font(Theme.Font.readoutValue).foregroundStyle(Theme.Color.signal)
        }
    }

    // MARK: Knobs

    private var knobCard: some View {
        let families: [(String, Knob.Family, Int, String)] = [
            ("Pitch", .pitch, 4, "+2 st"), ("Gain", .gain, -3, "-3.0 dB"), ("Filter", .filter, 6, "2.4 kHz"),
            ("Attack", .time, 2, "12 ms"), ("Start", .neutral, 0, "0 ms"),
        ]
        return card("Knob — parameter family") {
            HStack(alignment: .top, spacing: Theme.Space.s24) {
                ForEach(families, id: \.0) { name, family, value, text in
                    VStack(spacing: Theme.Space.s6) {
                        Text(name).font(Theme.Font.row)
                        Knob(
                            value: .constant(value), in: -12...12, accessibilityLabel: name,
                            family: family, size: .large)
                        Text(text).font(Theme.Font.readoutValue).foregroundStyle(.secondary)
                    }
                }
            }
            HStack(spacing: Theme.Space.s12) {
                ForEach(families.prefix(2), id: \.0) { name, family, value, text in
                    Knob(value: .constant(value), in: -12...12, accessibilityLabel: name, family: family)
                    Text(text).font(Theme.Font.readoutValue)
                }
                VStack(alignment: .leading) {
                    Text("In bars: 24 pt").font(Theme.Font.caption).foregroundStyle(.secondary)
                    Text("In panels: 44 pt").font(Theme.Font.caption).foregroundStyle(.secondary)
                }
            }
        }
    }

    // MARK: Transport, readout, rows

    private var transportCard: some View {
        card("Transport · value · row") {
            HStack(spacing: Theme.Space.s8) {
                Image(systemName: "play.fill").foregroundStyle(Color.white)
                    .frame(width: Theme.Size.transportButton, height: Theme.Size.transportButton)
                    .background(Theme.Color.controlAccent, in: Circle())
                Button {} label: { Image(systemName: "stop.fill") }
                Button {} label: { Image(systemName: "repeat") }
                Label("AUTO", systemImage: "repeat").font(Theme.Font.body).foregroundStyle(Theme.Color.signal)
                    .padding(.horizontal, Theme.Space.s8)
                    .frame(height: Theme.Size.transportButton)
                    .background(Theme.Color.signalSoft, in: RoundedRectangle(cornerRadius: Theme.Radius.medium))
            }
            HStack(spacing: Theme.Space.s16) {
                Readout("BPM", value: "138")
                Readout("Key", value: "Am")
                Readout("Length", value: "0:01.739")
            }
            .padding(Theme.Space.s8)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Theme.Color.waveStrip, in: RoundedRectangle(cornerRadius: Theme.Radius.well))
            Text("Selected + playing row")
                .font(Theme.Font.body)
                .padding(.horizontal, Theme.Space.s8)
                .frame(maxWidth: .infinity, minHeight: Theme.Size.rowHeight, alignment: .leading)
                .background(Theme.Color.selection, in: RoundedRectangle(cornerRadius: Theme.Radius.small))
            Text("Hover row")
                .font(Theme.Font.body)
                .padding(.horizontal, Theme.Space.s8)
                .frame(maxWidth: .infinity, minHeight: Theme.Size.rowHeight, alignment: .leading)
                .background(Theme.Color.selectionInactive, in: RoundedRectangle(cornerRadius: Theme.Radius.small))
            EmptyStateView(
                symbol: "magnifyingglass", title: "No results", message: "Nothing matches your search or filters.",
                actionTitle: "Clear Search & Filters", action: {}
            )
            .frame(height: 160)
        }
        .frame(width: 400)
    }

    // MARK: Swatches

    private var swatchCard: some View {
        card("Surfaces & colour") {
            ForEach(swatches, id: \.0) { name, color in
                HStack(spacing: Theme.Space.s8) {
                    RoundedRectangle(cornerRadius: Theme.Radius.small).fill(color)
                        .frame(width: Theme.Space.s32, height: Theme.Space.s16 + Theme.Space.s4)
                        .overlay(
                            RoundedRectangle(cornerRadius: Theme.Radius.small)
                                .strokeBorder(Theme.Color.hairline, lineWidth: Theme.hairlineWidth))
                    Text(name).font(Theme.Font.row)
                }
            }
        }
    }
}
