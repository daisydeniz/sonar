import SwiftUI

/// Simple wrapping layout for tag chips.
struct FlowLayout: Layout {
    var spacing: CGFloat = 6

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

/// Inspector tags: the focused row's tags as removable chips (removal applies
/// to the whole selection) and a field to add comma-separated tags.
@MainActor
struct TagsEditorView: View {
    @Bindable var model: AppModel
    let row: FileRow
    @State private var draft = ""

    var body: some View {
        Section("Tags") {
            if !row.tags.isEmpty {
                FlowLayout {
                    ForEach(row.tags, id: \.self) { tag in
                        HStack(spacing: 2) {
                            Text(tag)
                            Button {
                                Task { await model.removeTag(tag) }
                            } label: {
                                Image(systemName: "xmark.circle.fill")
                            }
                            .buttonStyle(.plain)
                            .accessibilityLabel("Remove tag \(tag)")
                        }
                        .padding(.horizontal, 8).padding(.vertical, 2)
                        .background(Capsule().fill(.quaternary))
                    }
                }
            }
            HStack {
                TextField("Add tag", text: $draft)
                    .onSubmit(commit)
                    .onChange(of: row.fileId) { draft = "" }
                    .accessibilityLabel("Add tag")
                let suggestions = model.organizeState.tags.filter { !row.tags.contains($0.name) }
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
