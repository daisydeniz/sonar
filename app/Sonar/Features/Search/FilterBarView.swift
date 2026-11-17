import SwiftUI

/// The filter bar (DESIGN §3): one 36 pt row on `surfaceRaised`. Kind as a
/// segmented control, Category / Key / BPM / Duration / Tags as popover chips with facet counts from the core
/// (drill-sideways), a Favorites icon toggle and the result count. Changing
/// anything re-queries the core through `AppModel` (the view only edits
/// `filters`).
@MainActor
struct FilterBarView: View {
    @Bindable var model: AppModel

    var body: some View {
        HStack(spacing: Theme.Space.s8) {
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: Theme.Space.s8) {
                    kindPicker
                    categoryChip
                    keyChip
                    bpmChip
                    durationChip
                    tagsChip
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

    private var categoryChip: some View {
        FilterPopoverChip(
            title: "Category",
            summary: FilterState.summary(of: model.filters.categories, display: Format.categoryTitle),
            onClear: { model.filters.categories = [] }
        ) {
            FacetListView(
                title: "Category",
                chips: FacetChip.chips(facets: model.facets?.categories ?? [], selected: model.filters.categories),
                display: Format.categoryTitle, dot: { Theme.categoryColor($0) }
            ) { FilterState.toggle($0, in: &model.filters.categories) }
        }
    }

    private var keyChip: some View {
        FilterPopoverChip(
            title: "Key",
            summary: FilterState.summary(of: model.filters.keys).map {
                model.filters.compatibleKeys ? "\($0) ≈" : $0
            },
            onClear: { model.filters.keys = []; model.filters.compatibleKeys = false }
        ) {
            VStack(alignment: .leading, spacing: Theme.Space.s8) {
                FacetListView(
                    title: "Key",
                    chips: FacetChip.chips(facets: model.facets?.keys ?? [], selected: model.filters.keys)
                ) { FilterState.toggle($0, in: &model.filters.keys) }
                Toggle("Include compatible keys", isOn: $model.filters.compatibleKeys)
                    .toggleStyle(.checkbox)
                    .font(Theme.Font.row)
                    .disabled(model.filters.keys.isEmpty)
            }
        }
    }

    private var bpmChip: some View {
        FilterPopoverChip(
            title: "BPM",
            summary: FilterState.rangeSummary(min: model.filters.bpmMin, max: model.filters.bpmMax),
            onClear: { model.filters.bpmMin = nil; model.filters.bpmMax = nil }
        ) {
            RangeFieldsView(title: "BPM", unit: "", min: $model.filters.bpmMin, max: $model.filters.bpmMax)
        }
    }

    private var durationChip: some View {
        FilterPopoverChip(
            title: "Duration",
            summary: FilterState.rangeSummary(
                min: model.filters.durationMinSec, max: model.filters.durationMaxSec, unit: "s"),
            onClear: { model.filters.durationMinSec = nil; model.filters.durationMaxSec = nil }
        ) {
            RangeFieldsView(
                title: "Duration", unit: "s", min: $model.filters.durationMinSec, max: $model.filters.durationMaxSec)
        }
    }

    private var tagsChip: some View {
        FilterPopoverChip(
            title: "Tags", summary: FilterState.summary(of: model.filters.tags),
            onClear: { model.filters.tags = [] }
        ) {
            FacetListView(
                title: "Tags", chips: FacetChip.chips(facets: model.facets?.tags ?? [], selected: model.filters.tags)
            ) { FilterState.toggle($0, in: &model.filters.tags) }
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
