import SwiftUI

/// Inspector for the selected row(s): metadata and editing only (DESIGN §3).
/// States: no selection (`EmptyStateView`), one sample (header, three equal
/// actions, cards Analysis / Tags / File) and several samples (count header,
/// batch actions, Analysis and Tags cards that apply to the whole selection).
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
                VStack(spacing: Theme.Space.s8) {
                    if selectionCount > 1 {
                        multiHeader(row)
                    } else {
                        header(row)
                    }
                    OverrideFieldsView(model: model, row: row)
                    TagsEditorView(model: model, row: row)
                    if selectionCount <= 1 { fileCard(row) }
                }
                .padding(.horizontal, Theme.Space.s12)
                .padding(.vertical, Theme.Space.s12)
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
                    .font(Theme.Font.caption).foregroundStyle(.tertiary)
                    .lineLimit(2).truncationMode(.middle).textSelection(.enabled)
            }
            HStack(spacing: Theme.Space.s8) {
                favoriteButton(row)
                Button { onFindSimilar(row) } label: {
                    equalLabel("Similar", systemImage: "waveform.badge.magnifyingglass")
                }
                .accessibilityLabel("Find Similar")
                .help("Find similar (⌘⇧F)")
                revealButton
            }
            .buttonStyle(.bordered).controlSize(.small)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.bottom, Theme.Space.s4)
    }

    private func multiHeader(_ row: FileRow) -> some View {
        VStack(alignment: .leading, spacing: Theme.Space.s8) {
            VStack(alignment: .leading, spacing: Theme.Space.s2) {
                Text("\(selectionCount) samples selected")
                    .font(Theme.Font.title).foregroundStyle(.primary)
                Text("Edits apply to all selected samples")
                    .font(Theme.Font.caption).foregroundStyle(.tertiary)
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
                    equalLabel("Add to", systemImage: "rectangle.stack.badge.plus")
                }
                .menuIndicator(.hidden)
                .accessibilityLabel("Add to Collection")
                .disabled(model.collections.isEmpty)
                .help("Add the selection to a collection")
                revealButton
            }
            .buttonStyle(.bordered).controlSize(.small)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.bottom, Theme.Space.s4)
    }

    /// Label that fills its share of the action row (three equal buttons).
    private func equalLabel(_ title: String, systemImage: String) -> some View {
        Label(title, systemImage: systemImage)
            .lineLimit(1).frame(maxWidth: .infinity)
    }

    private var revealButton: some View {
        Button { model.revealSelected() } label: { equalLabel("Reveal", systemImage: "folder") }
            .help("Reveal in Finder")
    }

    private func favoriteButton(_ row: FileRow) -> some View {
        let on = model.organizeTargetRows.isEmpty ? row.favorite : model.organizeTargetRows.allSatisfy(\.favorite)
        return Button { onToggleFavorite(row) } label: {
            equalLabel(on ? "Unfavorite" : "Favorite", systemImage: on ? "heart.fill" : "heart")
                .foregroundStyle(on ? Theme.Color.favorite : Color.primary)
        }
        .help("Favorite (⌘L)")
    }

    /// Folder of the file (its path without the file name).
    private func folderPath(_ row: FileRow) -> String {
        URL(fileURLWithPath: row.path).deletingLastPathComponent().path
    }

    // MARK: File card

    /// Bit depth and size are not in the core's `FileRow`, so the card lists
    /// what the core provides (ROADMAP `## Later`).
    private func fileCard(_ row: FileRow) -> some View {
        InspectorCard("File") {
            PropertyRow("Duration", value: Format.duration(ms: row.durationMs), numeric: true)
            PropertyRow("Sample rate", value: Format.sampleRate(row.sampleRate), numeric: true)
            PropertyRow("Channels", value: Format.channels(row.channels))
            PropertyRow("Library", value: model.libraries.first { $0.id == row.libraryId }
                .map { URL(fileURLWithPath: $0.rootPath).lastPathComponent })
            if row.hidden { PropertyRow("Hidden", value: "Yes") }
        }
    }
}
