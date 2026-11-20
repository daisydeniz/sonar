import SwiftUI

/// The live tag popover (DESIGN §4.12 layer 2): header, token field with the
/// current tags as chips and an inline field, then "Most used" or the
/// matches with "Create tag" last, and a footer with Manage Tags… and the
/// key hint. Every change applies at once (one Undo step each).
@MainActor
struct TagPopoverView: View {
    @Bindable var model: TagPopoverModel
    let onClose: () -> Void
    @FocusState private var fieldFocused: Bool

    private var state: TagPopoverState { model.state }

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.s8) {
            header
            tokenField.disabled(model.isLoading)
            list.disabled(model.isLoading)
            if state.chips.contains(where: { $0.mark != .all }), state.total > 1 { mixedNote }
            Hairline()
            footer
        }
        .padding(Theme.Space.s12)
        .frame(width: Theme.Size.tagPopoverWidth)
        .onAppear { DispatchQueue.main.async { fieldFocused = true } }
        .onChange(of: model.app.organizeState.tagsRevision) { model.reload() }
    }

    // MARK: Header

    private var header: some View {
        HStack(spacing: Theme.Space.s8) {
            Image(systemName: "tag").foregroundStyle(Theme.Hue.tag.color)
            Text("Tags").font(Theme.Font.body.weight(.bold))
            Text(model.subtitle)
                .font(Theme.Font.caption).foregroundStyle(.tertiary)
                .lineLimit(1).truncationMode(.middle)
            Spacer(minLength: 0)
            if model.isLoading { ProgressView().controlSize(.small) }
        }
        .padding(.horizontal, Theme.Space.s4)
    }

    // MARK: Token field

    private var tokenField: some View {
        FlowLayout {
            ForEach(state.chips, id: \.name) { chip in
                TagChipView(
                    chip: chip, mixedCaption: state.mixedCaption(chip.mark),
                    onAdd: { model.add(chip.name) }, onRemove: { model.remove(chip.name) })
            }
            TextField("Add tag…", text: $model.state.text)
                .textFieldStyle(.plain)
                .font(Theme.Font.row)
                .frame(minWidth: Theme.Size.tagFieldWidth, maxWidth: .infinity)
                .frame(height: Theme.Size.chipHeight)
                .focused($fieldFocused)
                .onChange(of: model.state.text) { model.textChanged() }
                .onSubmit { model.activateHighlighted() }
                .onKeyPress(.downArrow) { model.moveHighlight(by: 1); return .handled }
                .onKeyPress(.upArrow) { model.moveHighlight(by: -1); return .handled }
                .onKeyPress(.tab) {
                    guard !state.text.isEmpty else { return .ignored }
                    model.commitText()
                    return .handled
                }
                .onKeyPress(.delete) {
                    guard state.text.isEmpty else { return .ignored }
                    model.removeLast()
                    return .handled
                }
                .onExitCommand(perform: onClose)
                .accessibilityLabel("Add tag")
        }
        .frame(maxWidth: .infinity, minHeight: Theme.Size.tagBoxMinHeight, alignment: .topLeading)
        .padding(Theme.Space.s8)
        .background(Theme.Color.surfaceContent, in: RoundedRectangle(cornerRadius: Theme.Radius.medium))
        .overlay {
            RoundedRectangle(cornerRadius: Theme.Radius.medium)
                .strokeBorder(
                    fieldFocused ? Theme.Color.accent : Theme.Color.hairline,
                    lineWidth: fieldFocused ? Theme.Space.s2 : Theme.hairlineWidth)
        }
    }

    // MARK: List

    private var list: some View {
        let rows = state.rows
        let split = rows.firstIndex { if case .create = $0 { true } else { false } }
        return ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 0) {
                    if state.text.isEmpty {
                        Text("Most used").sonarLabelStyle().foregroundStyle(.secondary)
                            .padding(.horizontal, Theme.Space.s8)
                            .padding(.vertical, Theme.Space.s6)
                            .accessibilityAddTraits(.isHeader)
                    }
                    if rows.isEmpty {
                        Text("No tags yet. Type to create one.")
                            .font(Theme.Font.row).foregroundStyle(.tertiary)
                            .frame(maxWidth: .infinity, minHeight: Theme.Size.rowHeight)
                    }
                    ForEach(Array(rows.enumerated()), id: \.offset) { index, row in
                        if index == split, index > 0 { Hairline().padding(.horizontal, Theme.Space.s8) }
                        rowView(row, highlighted: state.highlight == index)
                            .id(index)
                            .onTapGesture { model.activate(row); fieldFocused = true }
                    }
                }
                .padding(.vertical, Theme.Space.s4)
            }
            .onChange(of: state.highlight) { if let h = state.highlight { proxy.scrollTo(h) } }
        }
        .frame(maxHeight: Theme.Size.tagListHeight)
        .fixedSize(horizontal: false, vertical: true)
    }

    private func rowView(_ row: TagPopoverState.Row, highlighted: Bool) -> some View {
        HStack(spacing: Theme.Space.s8) {
            switch row {
            case .create(let name):
                Image(systemName: "plus").imageScale(.small).foregroundStyle(Theme.Color.accent)
                    .frame(width: Theme.Size.tagCheckWidth)
                Text("Create tag “\(name)”").font(Theme.Font.body).lineLimit(1)
                Spacer(minLength: 0)
            case .tag(let name, let files, let mark):
                markView(mark).frame(width: Theme.Size.tagCheckWidth)
                Text(name).font(Theme.Font.body).lineLimit(1).truncationMode(.middle)
                Spacer(minLength: Theme.Space.s8)
                Text("\(files)").font(Theme.Font.captionNumeric).foregroundStyle(.secondary)
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

    @ViewBuilder private func markView(_ mark: TagMark) -> some View {
        switch mark {
        case .all: Image(systemName: "checkmark").imageScale(.small).foregroundStyle(Theme.Color.accent)
        case .some: Image(systemName: "minus").imageScale(.small).foregroundStyle(Theme.Color.accent)
        case .none: Color.clear
        }
    }

    private var mixedNote: some View {
        Text("Dashed: on some of the selected samples. Click to add to all.")
            .font(Theme.Font.caption).foregroundStyle(.tertiary)
            .fixedSize(horizontal: false, vertical: true)
            .padding(.horizontal, Theme.Space.s4)
    }

    // MARK: Footer

    private var footer: some View {
        HStack(spacing: Theme.Space.s8) {
            Button("Manage Tags…") { model.manage() }
                .buttonStyle(.link).font(Theme.Font.row)
                .disabled(!model.canManage)
            Spacer(minLength: 0)
            HStack(spacing: Theme.Space.s4) {
                Image(systemName: "return").imageScale(.small)
                Text("add · esc close")
            }
            .font(Theme.Font.caption).foregroundStyle(.tertiary)
            .accessibilityElement(children: .combine)
        }
        .padding(.horizontal, Theme.Space.s4)
    }
}
