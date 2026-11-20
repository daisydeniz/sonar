import SwiftUI

/// Inspector "Tags" section: the focused row's tags (with several rows
/// selected, the tags every selected row has) as removable chips (removal
/// applies to the whole selection) and a field to add comma-separated tags.
@MainActor
struct TagsEditorView: View {
    @Bindable var model: AppModel
    let row: FileRow
    @State private var draft = ""

    /// Tags shown as chips: the intersection across a multi-selection.
    private var shownTags: [String] {
        guard model.selectedFileIDs.count > 1 else { return row.tags }
        let rows = model.organizeTargetRows
        // Unloaded selected rows cannot be compared: show nothing uncertain.
        guard rows.count >= model.selectedFileIDs.count else { return [] }
        let common = commonTags(rows.map(\.tags))
        return row.tags.filter(common.contains)
    }

    var body: some View {
        InspectorCard("Tags") {
            Button { model.editTags() } label: { Image(systemName: "tag").imageScale(.small) }
                .buttonStyle(.borderless).foregroundStyle(.secondary)
                .help("Edit tags")
                .accessibilityLabel("Edit tags")
        } content: {
            if !shownTags.isEmpty {
                FlowLayout {
                    ForEach(shownTags, id: \.self) { tag in
                        SonarChip(title: tag, hue: .tag, trailingSymbol: "xmark") {
                            Task { await model.removeTag(tag) }
                        }
                        .help("Remove tag")
                        .accessibilityLabel("Remove tag \(tag)")
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            HStack(spacing: Theme.Space.s4) {
                TextField("Add tag", text: $draft)
                    .textFieldStyle(.roundedBorder)
                    .controlSize(.small)
                    .onSubmit(commit)
                    .onChange(of: row.fileId) { draft = "" }
                    .accessibilityLabel("Add tag")
            }
        }
    }

    private func commit() {
        let text = draft
        draft = ""
        Task { await model.addTags(text) }
    }
}
