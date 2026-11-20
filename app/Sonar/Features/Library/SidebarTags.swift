import SwiftUI

/// Sidebar Tags section (DESIGN §3, ADR-043): the 5 most used tags with the
/// `tag` symbol and a count badge, and a "Show All (n)" row that expands the
/// section in place. Clicking a tag scopes the results to it; dropping result
/// rows on a tag adds it to them (one Undo step).
@MainActor
struct SidebarTagsSection: View {
    let model: AppModel
    @State private var expanded = false

    var body: some View {
        let used = TagManageLogic.sidebarTags(model.organizeState.tags)
        let shown = TagManageLogic.sidebarVisible(
            model.organizeState.tags, expanded: expanded, selected: model.sidebarSelection.tagName)
        if !used.isEmpty {
            Section {
                ForEach(shown, id: \.name) { tag in
                    SidebarTagRow(model: model, tag: tag)
                }
                if used.count > TagManageLogic.sidebarLimit {
                    Button { expanded.toggle() } label: {
                        Label(expanded ? "Show Less" : "Show All (\(used.count))", systemImage: "ellipsis")
                            .foregroundStyle(.secondary)
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel(expanded ? "Show fewer tags" : "Show all \(used.count) tags")
                }
            } header: {
                Text("Tags")
            }
        }
    }
}

/// One tag row: scope selection and the drop target for result rows.
@MainActor
private struct SidebarTagRow: View {
    let model: AppModel
    let tag: TagInfo
    @State private var targeted = false

    var body: some View {
        Label(tag.name, systemImage: "tag")
            .badge(Int(tag.files))
            .tag(SidebarItem.tag(tag.name))
            .listRowBackground(
                targeted ? Theme.Color.selection.clipShape(RoundedRectangle(cornerRadius: Theme.Radius.medium)) : nil)
            .onDrop(of: [.fileURL], isTargeted: $targeted) { _ in
                // Only rows dragged out of the results table add the tag.
                guard DragOutState.isDraggingFromTable else { return false }
                Task { await model.dropRowsOnTag(tag.name) }
                return true
            }
            .help("Show samples tagged “\(tag.name)”. Drop samples here to add the tag.")
            .contextMenu {
                Button("Manage Tags…") { model.organizeState.showsManageTags = true }
            }
            .accessibilityLabel("\(tag.name), \(tag.files) samples")
    }
}
