import SwiftUI

/// The Filters popover (DESIGN §3 "Filter panel", ADR-042): sections as hued
/// chip clouds with counts. A click applies at once. Selection logic lives
/// in `FilterPanel`; this view only draws it and edits `model.filters`.
@MainActor
struct FilterPanelView: View {
    @Bindable var model: AppModel
    @Binding var keyMode: KeyMode
    var onClose: () -> Void = {}
    @State private var expanded: Set<String> = []

    /// More chips than this (about two rows) get a "Show More" button.
    private static let collapsedLimit = 12

    var body: some View {
        VStack(spacing: 0) {
            header
            ScrollView {
                VStack(spacing: Theme.Space.s8) {
                    quickCard
                    categorySection
                    keySection
                    tempoSection
                    durationSection
                    tagSection
                    librarySection
                }
                .padding(.horizontal, Theme.Space.s12)
            }
            .frame(maxHeight: Theme.Size.filterPanelMaxHeight)
            footer
        }
        .frame(width: Theme.Size.filterPanelWidth)
    }

    private var header: some View {
        ZStack {
            Text("Filters").font(Theme.Font.body.bold())
                .accessibilityAddTraits(.isHeader)
            HStack {
                Spacer()
                Button(action: onClose) {
                    Image(systemName: "xmark").imageScale(.small)
                        .frame(width: Theme.Size.panelCloseButton, height: Theme.Size.panelCloseButton)
                }
                .buttonStyle(.plain)
                .background(Theme.Color.surfaceGroup, in: Circle())
                .keyboardShortcut(.cancelAction)
                .accessibilityLabel("Close filters")
                .help("Close")
            }
        }
        .padding(Theme.Space.s12)
    }

    /// Quick toggles card (DESIGN §3): Favorites only.
    private var quickCard: some View {
        HStack {
            let on = model.filters.favoritesOnly
            Toggle(isOn: $model.filters.favoritesOnly) {
                Label(
                    "Favorites", systemImage: on ? "heart.fill" : "heart")
                    .foregroundStyle(on ? Theme.Color.favorite : Color.primary)
            }
            .toggleStyle(.button)
            .controlSize(.small)
            .accessibilityLabel("Favorites only")
            Spacer()
        }
        .padding(Theme.Space.s12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Theme.Color.surfaceGroup, in: RoundedRectangle(cornerRadius: Theme.Radius.large))
    }

    private var footer: some View {
        HStack {
            let n = FilterPanel.filterCount(model.filters)
            Text("\(model.resultCount.formatted()) results · \(n) \(n == 1 ? "filter" : "filters")")
                .font(Theme.Font.captionNumeric).foregroundStyle(.secondary)
            Spacer()
            Button("Clear all") { model.clearFilters() }
                .buttonStyle(.plain)
                .font(Theme.Font.row)
                .foregroundStyle(Theme.Color.accent)
                .disabled(!model.filters.isActive)
        }
        .padding(Theme.Space.s12)
    }

    // MARK: Sections

    private func section<Content: View>(
        _ title: String, hue: Theme.Hue, moreKey: String? = nil, total: Int = 0, @ViewBuilder content: () -> Content
    ) -> some View {
        VStack(alignment: .leading, spacing: Theme.Space.s8) {
            HStack {
                Rectangle().fill(hue.fill)
                    .frame(width: Theme.Size.hueSquare, height: Theme.Size.hueSquare)
                    .accessibilityHidden(true)
                Text(title).font(Theme.Font.body.weight(.semibold))
                    .accessibilityAddTraits(.isHeader)
                Spacer()
                if let moreKey, total > Self.collapsedLimit {
                    let open = expanded.contains(moreKey)
                    Button(open ? "Show Less" : "Show More") { FilterState.toggle(moreKey, in: &expanded) }
                        .buttonStyle(.plain)
                        .font(Theme.Font.caption)
                        .foregroundStyle(.secondary)
                }
            }
            content()
        }
        .padding(Theme.Space.s12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Theme.Color.surfaceGroup, in: RoundedRectangle(cornerRadius: Theme.Radius.large))
    }

    private func chip(
        _ title: String, count: Int? = nil, selected: Bool, hue: Theme.Hue, disabled: Bool = false,
        label: String? = nil, action: @escaping () -> Void
    ) -> some View {
        SonarChip(title: title, count: count, isActive: selected, hue: hue, action: action)
            .fixedSize()
            .disabled(disabled && !selected)
            .accessibilityLabel(label ?? title)
    }

    private var categorySection: some View {
        let all = FilterPanel.categoryChips(
            FacetChip.chips(facets: model.facets?.categories ?? [], selected: model.filters.categories))
        let shown = FilterPanel.visible(all, limit: Self.collapsedLimit, expanded: expanded.contains("category"))
        return section("Category", hue: .category, moreKey: "category", total: all.count) {
            if all.isEmpty {
                emptyHint("No categories yet")
            } else {
                FlowLayout {
                    ForEach(shown) { c in
                        chip(
                            Format.categoryTitle(c.value), count: Int(c.count), selected: c.selected,
                            hue: .category, disabled: c.count == 0,
                            label: "Category \(Format.categoryTitle(c.value)), \(c.count) results"
                        ) { FilterState.toggle(c.value, in: &model.filters.categories) }
                    }
                }
            }
        }
    }

    private var keySection: some View {
        let facets = model.facets?.keys ?? []
        return section("Key", hue: .key) {
            Picker("Mode", selection: $keyMode) {
                ForEach(KeyMode.allCases) { Text($0.title).tag($0) }
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .fixedSize()
            .accessibilityLabel("Key mode")
            FlowLayout {
                ForEach(FilterPanel.roots, id: \.self) { root in
                    let selected = FilterPanel.isKeySelected(root: root, mode: keyMode, in: model.filters.keys)
                    chip(
                        root, count: FilterPanel.keyCount(root: root, mode: keyMode, facets: facets),
                        selected: selected, hue: .key,
                        disabled: !FilterPanel.isKeyAvailable(
                            root: root, mode: keyMode, facets: facets, keys: model.filters.keys),
                        label: "Key \(root) \(keyMode.title)"
                    ) { FilterPanel.toggleKey(root: root, mode: keyMode, in: &model.filters.keys) }
                }
            }
            HStack(spacing: Theme.Space.s16) {
                Toggle("Include compatible keys", isOn: $model.filters.compatibleKeys)
                    .disabled(model.filters.keys.isEmpty)
                Toggle("Include uncertain", isOn: $model.filters.includeUncertainKeys)
                    .help("Also list keys the analysis is unsure about (low confidence)")
            }
            .toggleStyle(.checkbox)
            .font(Theme.Font.row)
        }
    }

    private var tempoSection: some View {
        section("Tempo", hue: .property) {
            rangeFields(title: "BPM", unit: "BPM", min: $model.filters.bpmMin, max: $model.filters.bpmMax)
            rangeChips(FilterPanel.tempoRanges, unit: "", min: $model.filters.bpmMin, max: $model.filters.bpmMax)
        }
    }

    private var durationSection: some View {
        section("Duration", hue: .property) {
            rangeFields(
                title: "Duration", unit: "s", min: $model.filters.durationMinSec, max: $model.filters.durationMaxSec)
            rangeChips(
                FilterPanel.durationRanges, unit: "s", min: $model.filters.durationMinSec,
                max: $model.filters.durationMaxSec)
        }
    }

    private var tagSection: some View {
        let all = FacetChip.chips(facets: model.facets?.tags ?? [], selected: model.filters.tags)
        let shown = FilterPanel.visible(all, limit: Self.collapsedLimit, expanded: expanded.contains("tags"))
        return section("Tags", hue: .tag, moreKey: "tags", total: all.count) {
            if all.isEmpty {
                emptyHint("No tags yet")
            } else {
                FlowLayout {
                    ForEach(shown) { t in
                        chip(
                            t.value, count: Int(t.count), selected: t.selected, hue: .tag, disabled: t.count == 0,
                            label: "Tag \(t.value), \(t.count) results"
                        ) { FilterState.toggle(t.value, in: &model.filters.tags) }
                    }
                }
            }
        }
    }

    private var librarySection: some View {
        let chips = FilterPanel.libraryChips(
            facets: model.facets?.libraries ?? [], selected: model.filters.libraryIds, name: model.libraryName)
        return section("Library", hue: .library) {
            if chips.isEmpty {
                emptyHint("No libraries yet")
            } else {
                FlowLayout {
                    ForEach(chips) { l in
                        chip(
                            l.title, count: Int(l.count), selected: l.selected, hue: .library, disabled: l.count == 0,
                            label: "Library \(l.title), \(l.count) results"
                        ) { FilterState.toggle(l.id, in: &model.filters.libraryIds) }
                    }
                }
            }
        }
    }

    // MARK: Pieces

    private func emptyHint(_ text: String) -> some View {
        Text(text).font(Theme.Font.caption).foregroundStyle(.tertiary)
    }

    private func rangeChips(
        _ ranges: [QuickRange], unit: String, min: Binding<Double?>, max: Binding<Double?>
    ) -> some View {
        let current = FilterPanel.selectedRange(in: ranges, min: min.wrappedValue, max: max.wrappedValue)
        return FlowLayout {
            ForEach(ranges) { r in
                chip(r.label(unit: unit), selected: r == current, hue: .property) {
                    var lo = min.wrappedValue, hi = max.wrappedValue
                    FilterPanel.toggleRange(r, min: &lo, max: &hi)
                    min.wrappedValue = lo
                    max.wrappedValue = hi
                }
            }
        }
    }

    private func rangeFields(
        title: String, unit: String, min: Binding<Double?>, max: Binding<Double?>
    ) -> some View {
        HStack(spacing: Theme.Space.s8) {
            TextField("min", value: min, format: .number)
                .frame(width: Theme.Size.rangeField)
                .accessibilityLabel("\(title) minimum")
            Text("–").foregroundStyle(.secondary)
            TextField("max", value: max, format: .number)
                .frame(width: Theme.Size.rangeField)
                .accessibilityLabel("\(title) maximum")
            if !unit.isEmpty { Text(unit).font(Theme.Font.caption).foregroundStyle(.secondary) }
        }
        .textFieldStyle(.roundedBorder)
        .controlSize(.small)
        .font(Theme.Font.rowNumeric)
    }
}
