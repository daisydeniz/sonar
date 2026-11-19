import SwiftUI

/// The filter bar (DESIGN §3): one 36 pt row on `surfaceRaised`. Kind as a
/// segmented control, one Filters button (popover panel), one hued active
/// chip per selected value with its own `×`, a Favorites icon toggle,
/// "Clear all" and the result count. Changing anything re-queries the core
/// through `AppModel` (the view only edits `filters`).
@MainActor
struct FilterBarView: View {
    @Bindable var model: AppModel
    @State private var panelShown = false
    @State private var keyMode = KeyMode.any

    var body: some View {
        HStack(spacing: Theme.Space.s8) {
            kindPicker
            filtersButton
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: Theme.Space.s4) {
                    ForEach(FilterPanel.activeChips(model.filters)) { chip in
                        SonarChip(
                            title: chip.title, isActive: true, hue: chip.hue,
                            onClear: { clear(chip) }, clearLabel: "Remove \(chip.title) filter"
                        ) { panelShown = true }
                        .fixedSize()
                        .accessibilityLabel("Filter \(chip.title)")
                    }
                }
                .padding(.vertical, Theme.Space.s2)
            }
            favoritesToggle
            if model.filters.isActive {
                Button("Clear all") { model.clearFilters() }
                    .buttonStyle(.plain)
                    .font(Theme.Font.row)
                    .foregroundStyle(.secondary)
                    .keyboardShortcut(.delete, modifiers: [.command, .shift])
                    .help("Clear all filters (⇧⌘⌫)")
            }
            Text(Self.countText(model.resultCount))
                .font(Theme.Font.captionNumeric).foregroundStyle(.secondary)
                .lineLimit(1).fixedSize()
                .accessibilityLabel("\(model.resultCount) results")
        }
        .controlSize(.small)
        .padding(.horizontal, Theme.Space.s12)
        .frame(height: Theme.Size.barHeight)
        .background(Theme.Color.surfaceRaised)
    }

    private func clear(_ chip: FilterPanel.ActiveChip) {
        var next = model.filters
        FilterPanel.clear(chip, in: &next)
        model.filters = next
    }

    static func countText(_ count: Int) -> String {
        count == 1 ? "1 result" : "\(count.formatted()) results"
    }

    // MARK: Controls

    private var kindPicker: some View {
        Picker(
            "Kind",
            selection: Binding(
                get: { KindFilter(kinds: model.filters.kinds) },
                set: { model.filters.kinds = $0.kinds })
        ) {
            ForEach(KindFilter.allCases) { Text($0.title).tag($0) }
        }
        .pickerStyle(.segmented)
        .labelsHidden()
        .fixedSize()
        .accessibilityLabel("Kind")
    }

    private var filtersButton: some View {
        Button { panelShown.toggle() } label: {
            Label("Filters", systemImage: "line.3.horizontal.decrease")
                .foregroundStyle(model.filters.isActive ? Theme.Color.accent : Color.secondary)
        }
        .help("Filters")
        .accessibilityLabel("Filters")
        .popover(isPresented: $panelShown, arrowEdge: .bottom) {
            FilterPanelView(model: model, keyMode: $keyMode) { panelShown = false }
        }
    }

    private var favoritesToggle: some View {
        let on = model.filters.favoritesOnly
        let count = model.facets.map { " (\($0.favorites))" } ?? ""
        return Toggle(isOn: $model.filters.favoritesOnly) {
            Image(systemName: on ? "heart.fill" : "heart")
                .foregroundStyle(on ? Theme.Color.favorite : Color.secondary)
        }
        .toggleStyle(.button)
        .accessibilityLabel("Favorites only")
        .help("Show favorites only\(count)")
    }
}
