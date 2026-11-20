import SwiftUI

/// "Edit Tags" sheet (T10.9, DESIGN §4.11): the tags every target file has as
/// removable chips in a well with an inline field, and below it the library's
/// tags filtered by the typed text. Save applies the diff in one refresh.
@MainActor
struct TagEditorView: View {
    @State var state: TagEditorState
    let fileCount: Int
    let onSave: (_ added: [String], _ removed: [String]) -> Void
    let onCancel: () -> Void
    @FocusState private var fieldFocused: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.s12) {
            HStack(spacing: Theme.Space.s8) {
                Image(systemName: "tag").foregroundStyle(.secondary)
                Text("Edit Tags").font(Theme.Font.title)
                Spacer()
                if fileCount > 1 {
                    Text("\(fileCount) samples").font(Theme.Font.caption).foregroundStyle(.secondary)
                }
            }
            tokenBox
            suggestionList
            HStack(spacing: Theme.Space.s8) {
                Spacer()
                Button("Cancel", role: .cancel, action: onCancel).keyboardShortcut(.cancelAction)
                Button("Save") { onSave(state.added, state.removed) }
                    .buttonStyle(.borderedProminent)
                    .keyboardShortcut(.return, modifiers: .command)
                    .disabled(!state.hasChanges)
            }
        }
        .padding(Theme.Space.s16)
        .frame(width: Theme.Size.tagEditorWidth)
        .onAppear { fieldFocused = true }
    }

    // MARK: Chips + field

    private var tokenBox: some View {
        FlowLayout {
            ForEach(state.chips, id: \.self) { tag in
                SonarChip(title: tag, hue: .tag, trailingSymbol: "xmark") { state.remove(tag) }
                    .help("Remove tag")
                    .accessibilityLabel("Remove tag \(tag)")
            }
            TextField("Add tag", text: $state.text)
                .textFieldStyle(.plain)
                .font(Theme.Font.row)
                .frame(width: Theme.Size.tagFieldWidth, height: Theme.Size.chipHeight)
                .focused($fieldFocused)
                .onChange(of: state.text) { state.textChanged() }
                .onSubmit {
                    // Empty field + changes: Return saves (the sheet's default action).
                    if state.text.isEmpty, state.hasChanges { onSave(state.added, state.removed) } else { state.activateHighlighted() }
                }
                .onKeyPress(.downArrow) { state.moveHighlight(by: 1); return .handled }
                .onKeyPress(.upArrow) { state.moveHighlight(by: -1); return .handled }
                .onKeyPress(.tab) {
                    guard !state.text.isEmpty else { return .ignored }
                    state.commitText()
                    return .handled
                }
                .onKeyPress(.delete) {
                    guard state.text.isEmpty else { return .ignored }
                    state.removeLast()
                    return .handled
                }
                .accessibilityLabel("Add tag")
        }
        .frame(maxWidth: .infinity, minHeight: Theme.Size.tagBoxMinHeight, alignment: .topLeading)
        .padding(Theme.Space.s8)
        .background(Theme.Color.surfaceGroup, in: RoundedRectangle(cornerRadius: Theme.Radius.medium))
        .overlay {
            RoundedRectangle(cornerRadius: Theme.Radius.medium)
                .strokeBorder(Theme.Color.hairline, lineWidth: Theme.hairlineWidth)
        }
    }

    // MARK: List

    private var suggestionList: some View {
        let rows = state.rows
        return ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(spacing: 0) {
                    if rows.isEmpty {
                        Text("No tags yet. Type to create one.")
                            .font(Theme.Font.row).foregroundStyle(.tertiary)
                            .frame(maxWidth: .infinity, minHeight: Theme.Size.rowHeight)
                    }
                    ForEach(Array(rows.enumerated()), id: \.offset) { index, row in
                        rowView(row, highlighted: state.highlight == index)
                            .id(index)
                            .onTapGesture { activate(row) }
                    }
                }
            }
            .onChange(of: state.highlight) { if let h = state.highlight { proxy.scrollTo(h) } }
        }
        .frame(height: Theme.Size.tagListHeight)
        .background(Theme.Color.surfaceContent, in: RoundedRectangle(cornerRadius: Theme.Radius.medium))
        .overlay {
            RoundedRectangle(cornerRadius: Theme.Radius.medium)
                .strokeBorder(Theme.Color.hairline, lineWidth: Theme.hairlineWidth)
        }
    }

    private func rowView(_ row: TagEditorState.Row, highlighted: Bool) -> some View {
        HStack(spacing: Theme.Space.s8) {
            switch row {
            case .create(let name):
                Image(systemName: "plus").imageScale(.small).foregroundStyle(Theme.Color.accent)
                    .frame(width: Theme.Size.tagCheckWidth)
                Text("Create “\(name)”").font(Theme.Font.row).lineLimit(1)
                Spacer(minLength: 0)
            case .existing(let name, let files, let chosen):
                Image(systemName: "checkmark").imageScale(.small).foregroundStyle(Theme.Color.accent)
                    .opacity(chosen ? 1 : 0).frame(width: Theme.Size.tagCheckWidth)
                Text(name).font(Theme.Font.row).lineLimit(1).truncationMode(.middle)
                Spacer(minLength: Theme.Space.s8)
                if files > 0 { Text("\(files)").font(Theme.Font.captionNumeric).foregroundStyle(.secondary) }
            }
        }
        .padding(.horizontal, Theme.Space.s8)
        .frame(height: Theme.Size.rowHeight)
        .background(highlighted ? Theme.Color.selection : Color.clear, in: RoundedRectangle(cornerRadius: Theme.Radius.small))
        .padding(.horizontal, Theme.Space.s4)
        .contentShape(Rectangle())
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(.isButton)
    }

    private func activate(_ row: TagEditorState.Row) {
        switch row {
        case .create(let name): state.add(name)
        case .existing(let name, _, _): state.toggle(name)
        }
        state.text = ""
        fieldFocused = true
    }
}
