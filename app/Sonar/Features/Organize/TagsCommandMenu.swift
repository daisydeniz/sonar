import SwiftUI

/// Edit > Tags › (DESIGN §4.12): the same entries as the row context menu
/// (8 most used with ✓ / – marks, Add Tag… ⌘T, Manage Tags…) for the selection.
@MainActor
struct TagsCommandMenu: View {
    let model: AppModel

    var body: some View {
        // Reading tagsRevision refreshes ✓ / – after an edit or Undo.
        let _ = model.organizeState.tagsRevision
        let entry = model.tagMenuEntry(for: model.organizeTargetIDs)
        let enabled = !model.organizeTargetIDs.isEmpty
        Menu("Tags") {
            ForEach(Array(entry.children.enumerated()), id: \.offset) { _, child in
                if child.isSeparator {
                    Divider()
                } else if child.shortcut != nil {
                    Button(child.title) { child.action?() }
                        .keyboardShortcut("t", modifiers: .command)
                        .disabled(child.action == nil || !enabled)
                } else {
                    Button { child.action?() } label: {
                        switch child.mark {
                        case .on: Label(child.title, systemImage: "checkmark")
                        case .mixed: Label(child.title, systemImage: "minus")
                        case .off: Text(child.title)
                        }
                    }
                    .disabled(child.action == nil || !enabled)
                }
            }
        }
        .disabled(!enabled)
    }
}
