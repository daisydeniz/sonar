import SwiftUI

/// Inspector "Tags" card (DESIGN §4.12): the tags of the selection as chips
/// with `×` (a tag on only some selected files is the dashed "n/m" chip) and
/// the `+` that opens the tag popover. All edits apply to the whole selection.
@MainActor
struct TagsEditorView: View {
    @Bindable var model: AppModel
    let row: FileRow

    private var popoverShown: Binding<Bool> {
        Binding(
            get: { model.organizeState.tagPopover?.anchor == .inspector },
            set: { if !$0 { model.organizeState.tagPopover = nil } })
    }

    var body: some View {
        let state = model.tagState(for: model.organizeTargetIDs)
        InspectorCard("Tags") {
            Button { model.showTagPopover(anchor: .inspector) } label: {
                Image(systemName: "plus").imageScale(.small)
                    .frame(width: Theme.Size.tagAddButton, height: Theme.Size.tagAddButton)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.borderless).foregroundStyle(.secondary)
            .help("Add or remove tags (⌘T)")
            .accessibilityLabel("Add tag")
            .popover(isPresented: popoverShown, arrowEdge: .bottom) {
                if let request = model.organizeState.tagPopover {
                    TagPopoverHost(model: model, ids: request.ids)
                }
            }
        } content: {
            if state.chips.isEmpty {
                Text("No tags").font(Theme.Font.caption).foregroundStyle(.tertiary)
            } else {
                FlowLayout {
                    ForEach(state.chips, id: \.name) { chip in
                        TagChipView(
                            chip: chip, mixedCaption: state.mixedCaption(chip.mark),
                            onAdd: { Task { await model.addTags(names: [chip.name]) } },
                            onRemove: { Task { await model.removeTag(chip.name) } })
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
    }
}

/// Owns the popover model for one presentation (inspector `+` or row anchor).
@MainActor
struct TagPopoverHost: View {
    let model: AppModel
    @State private var popover: TagPopoverModel

    init(model: AppModel, ids: [Int64]) {
        self.model = model
        _popover = State(initialValue: TagPopoverModel(app: model, ids: ids))
    }

    var body: some View {
        TagPopoverView(model: popover, onClose: { model.organizeState.tagPopover = nil })
    }
}
