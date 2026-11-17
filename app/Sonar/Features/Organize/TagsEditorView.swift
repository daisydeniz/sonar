import SwiftUI

/// Simple wrapping layout for tag chips.
struct FlowLayout: Layout {
    var spacing: CGFloat = Theme.Space.s6

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        arrange(width: proposal.width ?? .infinity, subviews: subviews).size
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        let result = arrange(width: bounds.width, subviews: subviews)
        for (i, origin) in result.origins.enumerated() {
            subviews[i].place(at: CGPoint(x: bounds.minX + origin.x, y: bounds.minY + origin.y), proposal: .unspecified)
        }
    }

    private func arrange(width: CGFloat, subviews: Subviews) -> (size: CGSize, origins: [CGPoint]) {
        var origins: [CGPoint] = []
        var x: CGFloat = 0, y: CGFloat = 0, rowHeight: CGFloat = 0, maxX: CGFloat = 0
        for sub in subviews {
            let size = sub.sizeThatFits(.unspecified)
            if x > 0, x + size.width > width {
                x = 0
                y += rowHeight + spacing
                rowHeight = 0
            }
            origins.append(CGPoint(x: x, y: y))
            x += size.width + spacing
            rowHeight = max(rowHeight, size.height)
            maxX = max(maxX, x - spacing)
        }
        return (CGSize(width: maxX, height: y + rowHeight), origins)
    }
}

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
        VStack(spacing: Theme.Space.s6) {
            SectionLabel("Tags")
            if !shownTags.isEmpty {
                FlowLayout {
                    ForEach(shownTags, id: \.self) { tag in
                        SonarChip(title: tag, trailingSymbol: "xmark") {
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
                let suggestions = model.organizeState.tags.filter { !shownTags.contains($0.name) }
                if !suggestions.isEmpty {
                    Menu {
                        ForEach(suggestions, id: \.name) { info in
                            Button("\(info.name) (\(info.files))") {
                                Task { await model.addTags(names: [info.name]) }
                            }
                        }
                    } label: {
                        Image(systemName: "tag")
                    }
                    .menuStyle(.borderlessButton).fixedSize()
                    .controlSize(.small)
                    .help("Existing tags")
                    .accessibilityLabel("Existing tags")
                }
            }
        }
    }

    private func commit() {
        let text = draft
        draft = ""
        Task { await model.addTags(text) }
    }
}
