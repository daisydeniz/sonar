import SwiftUI
@testable import Sonar

/// Every design token and shared component on one page (UI snapshot `components`).
struct ComponentsGallery: View {
    private let swatches: [(String, Color)] = [
        ("accent", Theme.Color.accent), ("controlAccent", Theme.Color.controlAccent), ("surfaceContent", Theme.Color.surfaceContent),
        ("surfaceRaised", Theme.Color.surfaceRaised), ("surfaceWell", Theme.Color.surfaceWell),
        ("hairline", Theme.Color.hairline), ("favorite", Theme.Color.favorite),
        ("catDrum", Theme.Color.catDrum), ("catMetal", Theme.Color.catMetal), ("catBass", Theme.Color.catBass),
        ("catTonal", Theme.Color.catTonal), ("catVocal", Theme.Color.catVocal), ("catFX", Theme.Color.catFX),
        ("selection", Theme.Color.selection), ("selectionInactive", Theme.Color.selectionInactive),
        ("accentSoft", Theme.Color.accentSoft), ("error", Theme.Color.error),
        ("warning", Theme.Color.warning), ("ok", Theme.Color.ok),
    ]

    var body: some View {
        HStack(alignment: .top, spacing: Theme.Space.s24) {
            VStack(alignment: .leading, spacing: Theme.Space.s8) {
                SectionLabel("Colour tokens")
                LazyVGrid(
                    columns: Array(repeating: GridItem(.fixed(130), spacing: Theme.Space.s8), count: 3),
                    alignment: .leading, spacing: Theme.Space.s8
                ) {
                    ForEach(swatches, id: \.0) { name, color in
                        VStack(alignment: .leading, spacing: Theme.Space.s4) {
                            RoundedRectangle(cornerRadius: Theme.Radius.small).fill(color)
                                .frame(height: 40)
                                .overlay(
                                    RoundedRectangle(cornerRadius: Theme.Radius.small)
                                        .strokeBorder(Theme.Color.hairline, lineWidth: Theme.hairlineWidth))
                            Text(name).font(Theme.Font.caption).foregroundStyle(.secondary)
                        }
                    }
                }
                SectionLabel("Type scale")
                Text("Title 15 semibold").font(Theme.Font.title)
                Text("Body 13 regular").font(Theme.Font.body)
                Text("Row 12 regular  128.0 BPM  0:03.4").font(Theme.Font.row)
                Text("Row numeric 1234567").font(Theme.Font.rowNumeric)
                Text("Caption 11 regular").font(Theme.Font.caption)
                Text("Label style").sonarLabelStyle().foregroundStyle(.secondary)
            }
            VStack(alignment: .leading, spacing: Theme.Space.s8) {
                SectionLabel("SonarChip")
                HStack(spacing: Theme.Space.s8) {
                    SonarChip(title: "Default", action: {})
                    SonarChip(title: "Kick", count: 42, dot: Theme.categoryColor("kick"), action: {})
                    SonarChip(title: "Active", count: 7, isActive: true, action: {})
                    SonarChip(title: "Disabled", action: {}).disabled(true)
                }
                HStack(spacing: Theme.Space.s8) {
                    ForEach(["kick", "hat", "bass", "pad", "vocal", "fx"], id: \.self) {
                        SonarChip(title: $0, dot: Theme.categoryColor($0), action: {})
                    }
                }
                SectionLabel("Readout")
                HStack(spacing: Theme.Space.s24) {
                    Readout("BPM", value: "128.0")
                    Readout("Key", value: "F#m")
                    Readout("Length", value: "0:03.4")
                    Readout("Pitch", value: nil)
                }
                SectionLabel("Knob")
                HStack(spacing: Theme.Space.s24) {
                    Knob(value: .constant(0), in: -12...12, accessibilityLabel: "Default")
                    Knob(value: .constant(7), in: -12...12, accessibilityLabel: "Up")
                    Knob(value: .constant(-12), in: -12...12, accessibilityLabel: "Min")
                }
                SectionLabel("Hairline")
                Hairline()
                SectionLabel("EmptyStateView")
                EmptyStateView(
                    symbol: "magnifyingglass", title: "No results", message: "Nothing matches your search or filters.",
                    actionTitle: "Clear Search & Filters", action: {}
                )
                .frame(height: 200)
                .background(Theme.Color.surfaceRaised, in: RoundedRectangle(cornerRadius: Theme.Radius.large))
            }
            .frame(width: 440)
        }
        .padding(Theme.Space.s24)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .background(Theme.Color.surfaceContent)
    }
}
