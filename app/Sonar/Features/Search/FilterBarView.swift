import SwiftUI

/// Filter chips above the results: category, kind, key (+ compatible keys),
/// library, tags, favourites, BPM and duration ranges, and the result total.
/// Counts come from the core's facets (drill-sideways); changing anything
/// re-queries the core through `AppModel` (the view only edits `filters`).
@MainActor
struct FilterBarView: View {
    @Bindable var model: AppModel

    var body: some View {
        let facets = model.facets
        VStack(alignment: .leading, spacing: 6) {
            chipRow("Category", FacetChip.chips(facets: facets?.categories ?? [], selected: model.filters.categories)) {
                FilterState.toggle($0, in: &model.filters.categories)
            }
            chipRow("Kind", FacetChip.chips(facets: facets?.kinds ?? [], selected: model.filters.kinds)) {
                FilterState.toggle($0, in: &model.filters.kinds)
            }
            HStack(spacing: 8) {
                chipRow("Key", FacetChip.chips(facets: facets?.keys ?? [], selected: model.filters.keys)) {
                    FilterState.toggle($0, in: &model.filters.keys)
                }
                if !model.filters.keys.isEmpty {
                    Toggle("Compatible keys", isOn: $model.filters.compatibleKeys)
                        .toggleStyle(.checkbox)
                        .fixedSize()
                }
            }
            if model.showsLibraryChips {
                chipRow("Library", libraryChips(facets)) { id in
                    if let id = Int64(id) { FilterState.toggle(id, in: &model.filters.libraryIds) }
                }
            }
            if let tags = facets?.tags, !tags.isEmpty || !model.filters.tags.isEmpty {
                chipRow("Tags", FacetChip.chips(facets: tags, selected: model.filters.tags)) {
                    FilterState.toggle($0, in: &model.filters.tags)
                }
            }
            HStack(spacing: 12) {
                Toggle(isOn: $model.filters.favoritesOnly) {
                    Label(favoritesTitle(facets), systemImage: "heart")
                }
                .toggleStyle(.button)
                .accessibilityLabel("Favorites only")
                rangeFields("BPM", min: $model.filters.bpmMin, max: $model.filters.bpmMax)
                rangeFields("Duration (s)", min: $model.filters.durationMinSec, max: $model.filters.durationMaxSec)
                Spacer()
                if model.filters.isActive {
                    Button("Clear filters") { model.clearFilters() }
                        .keyboardShortcut(.delete, modifiers: [.command, .shift])
                }
                Text("\(model.resultCount.formatted()) results")
                    .monospacedDigit().foregroundStyle(.secondary)
                    .accessibilityLabel("\(model.resultCount) results")
            }
        }
        .controlSize(.small)
        .padding(.horizontal, 10).padding(.vertical, 6)
    }

    private func favoritesTitle(_ facets: SearchFacets?) -> String {
        facets.map { "Favorites (\($0.favorites))" } ?? "Favorites"
    }

    private func libraryChips(_ facets: SearchFacets?) -> [FacetChip] {
        let names = Dictionary(uniqueKeysWithValues: model.libraries.map {
            ($0.id, URL(fileURLWithPath: $0.rootPath).lastPathComponent)
        })
        let counts = Dictionary(uniqueKeysWithValues: (facets?.libraries ?? []).map { ($0.libraryId, $0.count) })
        return model.libraries.map { lib in
            // `value` carries the id (for toggling); the title is looked up by `chipTitle`.
            FacetChip(value: String(lib.id), count: counts[lib.id] ?? 0,
                      selected: model.filters.libraryIds.contains(lib.id))
        }.sorted { names[Int64($0.value) ?? 0] ?? "" < names[Int64($1.value) ?? 0] ?? "" }
    }

    private func title(_ label: String, _ chip: FacetChip) -> String {
        if label == "Library", let id = Int64(chip.value),
            let lib = model.libraries.first(where: { $0.id == id })
        {
            return URL(fileURLWithPath: lib.rootPath).lastPathComponent
        }
        return chip.value
    }

    @ViewBuilder
    private func chipRow(_ label: String, _ chips: [FacetChip], toggle: @escaping (String) -> Void) -> some View {
        if !chips.isEmpty {
            HStack(spacing: 6) {
                Text(label).foregroundStyle(.secondary).frame(width: 58, alignment: .leading)
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 4) {
                        ForEach(chips) { chip in
                            Toggle(isOn: Binding(get: { chip.selected }, set: { _ in toggle(chip.value) })) {
                                Text("\(title(label, chip)) \(chip.count)")
                            }
                            .toggleStyle(.button)
                            .opacity(chip.count == 0 && !chip.selected ? 0.5 : 1)
                            .accessibilityLabel("\(label) \(title(label, chip)), \(chip.count) results")
                        }
                    }
                }
            }
        }
    }

    private func rangeFields(_ label: String, min: Binding<Double?>, max: Binding<Double?>) -> some View {
        HStack(spacing: 4) {
            Text(label).foregroundStyle(.secondary)
            TextField("min", value: min, format: .number).frame(width: 54)
                .accessibilityLabel("\(label) minimum")
            Text("–")
            TextField("max", value: max, format: .number).frame(width: 54)
                .accessibilityLabel("\(label) maximum")
        }
        .textFieldStyle(.roundedBorder)
    }
}
