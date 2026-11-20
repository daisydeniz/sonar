import SwiftUI

/// Presents the Manage Tags sheet with its own model (reloaded on open).
@MainActor
struct TagManageSheet: View {
    let app: AppModel
    @State private var model: TagManageModel?

    var body: some View {
        Group {
            if let model {
                TagManageView(model: model, onDone: { app.organizeState.showsManageTags = false })
            }
        }
        .onAppear { if model == nil { model = TagManageModel(app: app) } }
    }
}

/// Manage Tags (DESIGN §4.12 layer 3): title, subtitle and filter, the
/// similar-tags card, the list of all tags (multi-select, double-click to
/// rename) and the bottom bar `+` `−` · hint · Merge… · Done.
@MainActor
struct TagManageView: View {
    @Bindable var model: TagManageModel
    let onDone: () -> Void
    @FocusState private var renameFocused: Bool
    @FocusState private var createFocused: Bool

    var body: some View {
        VStack(spacing: 0) {
            header
            if model.phase == .ready, !model.pairs.isEmpty {
                TagManageSimilarCard(model: model)
                    .padding([.horizontal, .bottom], Theme.Space.s16)
            }
            content
            Hairline()
            bottomBar
        }
        .frame(width: Theme.Size.manageTagsWidth)
        .frame(minHeight: Theme.Size.manageTagsMinHeight, idealHeight: Theme.Size.manageTagsMaxHeight,
            maxHeight: Theme.Size.manageTagsMaxHeight)
        .task { await model.reload() }
        .onChange(of: model.app.organizeState.tagsRevision) { Task { await model.reload() } }
        .confirmationDialog(model.deleteTitle, isPresented: $model.confirmingDelete) {
            Button("Delete", role: .destructive) { Task { await model.deleteSelection() } }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text(model.deleteMessage)
        }
    }

    // MARK: Header

    private var header: some View {
        HStack(alignment: .top, spacing: Theme.Space.s12) {
            VStack(alignment: .leading, spacing: Theme.Space.s2) {
                Text("Manage Tags").font(Theme.Font.title)
                Text(model.subtitle).font(Theme.Font.body).foregroundStyle(.secondary)
            }
            Spacer(minLength: 0)
            TextField("Filter", text: $model.filter, prompt: Text("Filter"))
                .textFieldStyle(.roundedBorder)
                .frame(width: Theme.Size.manageTagsFilterWidth)
                .accessibilityLabel("Filter tags")
        }
        .padding(Theme.Space.s16)
    }

    // MARK: Content

    @ViewBuilder private var content: some View {
        switch model.phase {
        case .loading:
            ProgressView("Loading tags…").frame(maxWidth: .infinity, maxHeight: .infinity)
        case .failed(let message):
            EmptyStateView(
                symbol: "exclamationmark.triangle", title: "Couldn't load tags", message: message,
                actionTitle: "Try Again", action: { Task { await model.reload() } })
        case .ready where model.tags.isEmpty && !model.isCreating:
            EmptyStateView(
                symbol: "tag", title: "No tags yet",
                message: "Select samples and press ⌘T to give them a tag.")
        case .ready:
            list
        }
    }

    private var list: some View {
        VStack(spacing: 0) {
            HStack {
                Text("Tag").sonarLabelStyle()
                Spacer(minLength: 0)
                Text("Samples").sonarLabelStyle()
            }
            .foregroundStyle(.tertiary)
            .padding(.horizontal, Theme.Space.s16)
            .frame(height: Theme.Size.headerHeight)
            Hairline()
            List(selection: $model.selection) {
                if model.isCreating { createRow }
                ForEach(model.visibleTags, id: \.name) { tag in
                    row(tag).tag(tag.name)
                }
            }
            .listStyle(.plain)
            .scrollContentBackground(.hidden)
            .overlay {
                if model.visibleTags.isEmpty, !model.isCreating {
                    Text("No tags match “\(model.filter)”")
                        .font(Theme.Font.row).foregroundStyle(.secondary)
                }
            }
        }
    }

    private func row(_ tag: TagInfo) -> some View {
        HStack(spacing: Theme.Space.s8) {
            if model.renaming == tag.name {
                TextField("Tag name", text: $model.renameText)
                    .textFieldStyle(.roundedBorder)
                    .focused($renameFocused)
                    .onSubmit { Task { await model.commitRename() } }
                    .onExitCommand { model.cancelRename() }
                    .onAppear { DispatchQueue.main.async { renameFocused = true } }
                    .accessibilityLabel("New name of \(tag.name)")
            } else {
                Text(tag.name).font(Theme.Font.body).lineLimit(1).truncationMode(.middle)
            }
            Spacer(minLength: Theme.Space.s8)
            if tag.files == 0 {
                Text("unused").font(Theme.Font.body).foregroundStyle(.tertiary)
            } else {
                Text("\(tag.files)").font(Theme.Font.rowNumeric).foregroundStyle(.secondary)
            }
        }
        .frame(height: Theme.Size.rowHeight)
        .contentShape(Rectangle())
        .onTapGesture(count: 2) { model.beginRename(tag.name) }
        .listRowSeparator(.hidden)
        .listRowInsets(rowInsets)
        .accessibilityElement(children: .combine)
        .accessibilityLabel("\(tag.name), \(tag.files == 0 ? "unused" : "\(tag.files) samples")")
        .accessibilityHint("Double-click to rename")
    }

    private var rowInsets: EdgeInsets {
        EdgeInsets(top: 0, leading: Theme.Space.s12, bottom: 0, trailing: Theme.Space.s12)
    }

    private var createRow: some View {
        TextField("New tag name", text: $model.createText)
            .textFieldStyle(.roundedBorder)
            .focused($createFocused)
            .onSubmit { Task { await model.commitCreate() } }
            .onExitCommand { model.isCreating = false }
            .onAppear { DispatchQueue.main.async { createFocused = true } }
            .frame(height: Theme.Size.rowHeight)
            .listRowSeparator(.hidden)
            .listRowInsets(rowInsets)
            .accessibilityLabel("New tag name")
    }

    // MARK: Bottom bar

    private var bottomBar: some View {
        HStack(spacing: Theme.Space.s8) {
            Button { model.beginCreate() } label: { Image(systemName: "plus") }
                .disabled(!model.canCreate)
                .help(model.canCreate ? "Add a new tag to the selected samples" : "Select samples in the results to add a tag")
                .accessibilityLabel("Add tag")
            Button { model.confirmingDelete = true } label: { Image(systemName: "minus") }
                .disabled(!model.canDelete)
                .help("Delete the selected tags")
                .accessibilityLabel("Delete selected tags")
            Text(model.hint).font(Theme.Font.body).foregroundStyle(.tertiary)
                .lineLimit(1)
                .padding(.leading, Theme.Space.s4)
            Spacer(minLength: Theme.Space.s8)
            Menu("Merge…") {
                ForEach(model.selectedTags, id: \.name) { tag in
                    Button("Merge into “\(tag.name)”") { Task { await model.mergeSelection(into: tag.name) } }
                }
            }
            .menuIndicator(.hidden)
            .fixedSize()
            .disabled(!model.canMerge)
            .help("Merge the selected tags into one: choose the name that stays")
            Button("Done", action: onDone).keyboardShortcut(.defaultAction)
        }
        .padding(Theme.Space.s16)
    }
}
