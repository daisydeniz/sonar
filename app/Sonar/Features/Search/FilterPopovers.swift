import SwiftUI

/// A filter-bar popover button: a default `SonarChip` with a chevron while
/// nothing is selected, an active chip (summary + clear ×) otherwise
/// (DESIGN §3 "Filter bar", §4.2).
@MainActor
struct FilterPopoverChip<Content: View>: View {
    let title: String
    /// Selection summary; nil = no selection.
    let summary: String?
    let onClear: () -> Void
    @ViewBuilder let content: () -> Content

    @State private var shown = false

    var body: some View {
        SonarChip(
            title: summary ?? title, isActive: summary != nil,
            trailingSymbol: summary == nil ? "chevron.down" : nil,
            onClear: onClear, clearLabel: "Clear \(title) filter"
        ) { shown.toggle() }
        .fixedSize()
        .popover(isPresented: $shown, arrowEdge: .bottom) {
            content()
                .padding(Theme.Space.s12)
                .frame(width: Theme.Size.popoverWidth)
        }
        .accessibilityLabel(summary.map { "\(title): \($0)" } ?? title)
        .help(title)
    }
}

/// Checkbox list of facet values with counts (caption, monospaced digits).
/// Zero-count values stay listed (dimmed) so a selection is never hidden.
struct FacetListView: View {
    let title: String
    let chips: [FacetChip]
    var display: (String) -> String = { $0 }
    /// Leading category dot colour for a value (categories only).
    var dot: (String) -> Color? = { _ in nil }
    let toggle: (String) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.s6) {
            Text(title).sonarLabelStyle().foregroundStyle(.secondary)
            if chips.isEmpty {
                Text("No values").font(Theme.Font.caption).foregroundStyle(.tertiary)
            } else {
                ScrollView {
                    VStack(alignment: .leading, spacing: Theme.Space.s2) {
                        ForEach(chips) { chip in row(chip) }
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
                .frame(maxHeight: Theme.Size.popoverListMaxHeight)
            }
        }
    }

    private func row(_ chip: FacetChip) -> some View {
        Toggle(isOn: Binding(get: { chip.selected }, set: { _ in toggle(chip.value) })) {
            HStack(spacing: Theme.Space.s6) {
                if let color = dot(chip.value) {
                    Circle().fill(color).frame(width: Theme.Size.categoryDot, height: Theme.Size.categoryDot)
                }
                Text(display(chip.value)).font(Theme.Font.row).lineLimit(1)
                Spacer(minLength: Theme.Space.s8)
                Text("\(chip.count)").font(Theme.Font.captionNumeric).foregroundStyle(.secondary)
            }
            .foregroundStyle(chip.count == 0 && !chip.selected ? Color.secondary : Color.primary)
        }
        .toggleStyle(.checkbox)
        .controlSize(.small)
        .accessibilityLabel("\(title) \(display(chip.value)), \(chip.count) results")
    }
}

/// Min / max number fields for BPM and duration.
struct RangeFieldsView: View {
    let title: String
    let unit: String
    @Binding var min: Double?
    @Binding var max: Double?

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.s6) {
            Text(title).sonarLabelStyle().foregroundStyle(.secondary)
            HStack(spacing: Theme.Space.s8) {
                TextField("min", value: $min, format: .number)
                    .frame(width: Theme.Size.rangeField)
                    .accessibilityLabel("\(title) minimum")
                Text("–").foregroundStyle(.secondary)
                TextField("max", value: $max, format: .number)
                    .frame(width: Theme.Size.rangeField)
                    .accessibilityLabel("\(title) maximum")
                if !unit.isEmpty { Text(unit).font(Theme.Font.caption).foregroundStyle(.secondary) }
            }
            .textFieldStyle(.roundedBorder)
            .controlSize(.small)
            .font(Theme.Font.rowNumeric)
        }
    }
}
