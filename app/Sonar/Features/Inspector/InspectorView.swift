import SwiftUI

/// Inspector for the selected row(s): metadata and editing only (DESIGN §3).
/// States: no selection (`EmptyStateView`), one sample (header, actions,
/// Analysis, Tags, File) and several samples (count header, batch actions,
/// Analysis and Tags edits that apply to the whole selection).
@MainActor
struct InspectorView: View {
    let model: AppModel
    let row: FileRow?
    let onToggleFavorite: (FileRow) -> Void
    let onFindSimilar: (FileRow) -> Void

    private var selectionCount: Int { model.selectedFileIDs.count }

    var body: some View {
        if let row {
            ScrollView {
                VStack(spacing: 0) {
                    if selectionCount > 1 {
                        multiHeader(row)
                    } else {
                        header(row)
                    }
                    OverrideFieldsView(model: model, row: row)
                    TagsEditorView(model: model, row: row)
                    if selectionCount <= 1 { fileSection(row) }
                }
                .padding(.horizontal, Theme.Space.s12)
                .padding(.bottom, Theme.Space.s16)
            }
        } else {
            EmptyStateView(
                symbol: "sidebar.trailing", title: "No selection",
                message: "Select a sample to see and edit its details.")
        }
    }

    // MARK: Header and actions

    private func header(_ row: FileRow) -> some View {
        VStack(alignment: .leading, spacing: Theme.Space.s8) {
            VStack(alignment: .leading, spacing: Theme.Space.s2) {
                Text(row.name)
                    .font(Theme.Font.title).foregroundStyle(.primary)
                    .lineLimit(2).textSelection(.enabled)
                Text(folderPath(row))
                    .font(Theme.Font.caption).foregroundStyle(.secondary)
                    .lineLimit(2).truncationMode(.middle).textSelection(.enabled)
            }
            HStack(spacing: Theme.Space.s8) {
                favoriteButton(row)
                Button { onFindSimilar(row) } label: {
                    Label("Similar", systemImage: "waveform.badge.magnifyingglass")
                }
                .accessibilityLabel("Find Similar")
                .help("Find similar (⌘⇧F)")
                Button { model.revealSelected() } label: {
                    Label("Reveal", systemImage: "folder")
                }
                .help("Reveal in Finder")
            }
            .buttonStyle(.bordered).controlSize(.small)
        }
        .padding(.top, Theme.Space.s12)
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private func multiHeader(_ row: FileRow) -> some View {
        VStack(alignment: .leading, spacing: Theme.Space.s8) {
            VStack(alignment: .leading, spacing: Theme.Space.s2) {
                Text("\(selectionCount) samples selected")
                    .font(Theme.Font.title).foregroundStyle(.primary)
                Text("Edits apply to all selected samples")
                    .font(Theme.Font.caption).foregroundStyle(.secondary)
            }
            HStack(spacing: Theme.Space.s8) {
                favoriteButton(row)
                Menu {
                    ForEach(model.collections, id: \.id) { collection in
                        Button(collection.name) {
                            let ids = model.organizeTargetIDs
                            Task { await model.addToCollection(id: collection.id, fileIds: ids) }
                        }
                    }
                } label: {
                    Label("Collection", systemImage: "rectangle.stack.badge.plus")
                }
                .labelStyle(.iconOnly)
                .accessibilityLabel("Add to Collection")
                .disabled(model.collections.isEmpty)
                .help("Add the selection to a collection")
                Button { model.revealSelected() } label: {
                    Label("Reveal", systemImage: "folder")
                }
                .help("Reveal in Finder")
            }
            .buttonStyle(.bordered).controlSize(.small)
        }
        .padding(.top, Theme.Space.s12)
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private func favoriteButton(_ row: FileRow) -> some View {
        let on = model.organizeTargetRows.isEmpty ? row.favorite : model.organizeTargetRows.allSatisfy(\.favorite)
        return Button { onToggleFavorite(row) } label: {
            Label(on ? "Unfavorite" : "Favorite", systemImage: on ? "heart.fill" : "heart")
                .foregroundStyle(on ? Theme.Color.favorite : Color.primary)
        }
        .help("Favorite (⌘L)")
    }

    /// Folder of the file (its path without the file name).
    private func folderPath(_ row: FileRow) -> String {
        URL(fileURLWithPath: row.path).deletingLastPathComponent().path
    }

    // MARK: File section

    private func fileSection(_ row: FileRow) -> some View {
        VStack(spacing: Theme.Space.s2) {
            SectionLabel("File")
            PropertyRow("Duration", value: Format.duration(ms: row.durationMs), numeric: true)
            PropertyRow("Sample rate", value: row.sampleRate.map { "\($0) Hz" }, numeric: true)
            PropertyRow("Channels", value: row.channels.map { $0 == 1 ? "Mono" : $0 == 2 ? "Stereo" : "\($0)" })
            PropertyRow("Library", value: model.libraries.first { $0.id == row.libraryId }
                .map { URL(fileURLWithPath: $0.rootPath).lastPathComponent })
            if row.hidden { PropertyRow("Hidden", value: "Yes") }
        }
    }
}
