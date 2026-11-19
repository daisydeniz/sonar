import SwiftUI

/// Inspector "Analysis" section: BPM / Key / Category / Kind. Editing writes a
/// user override for the whole selection (the core keeps it across
/// re-analysis); "Revert to detected" clears it. A value the core rejects
/// shows its message inline and the field reverts.
@MainActor
struct OverrideFieldsView: View {
    @Bindable var model: AppModel
    let row: FileRow
    @State private var bpmDraft = ""
    @State private var keyDraft = ""
    @State private var editingKey = false
    @FocusState private var keyFocused: Bool

    /// Rows the edits apply to (the multi-selection, else the focused row).
    private var targets: [FileRow] {
        let rows = model.organizeTargetRows
        return rows.isEmpty ? [row] : rows
    }
    /// Selected rows that are not loaded cannot be compared: treat as mixed.
    private var partial: Bool { model.organizeTargetRows.count < model.selectedFileIDs.count }
    private var bpm: CommonValue<Double> { CommonValue(targets.map(\.bpm), forceMixed: partial) }
    private var key: CommonValue<String> { CommonValue(targets.map(\.key), forceMixed: partial) }
    private var category: CommonValue<String> { CommonValue(targets.map(\.category), forceMixed: partial) }
    private var kind: CommonValue<String> { CommonValue(targets.map(\.kind), forceMixed: partial) }

    /// One sample selected: key provenance and note are per file.
    private var single: Bool { targets.count == 1 && !partial }

    /// Source and confidence word, the name's key when it disagrees with the
    /// audio (ADR-041); a one-shot's note is in the Key row itself.
    @ViewBuilder private var keyDetailRows: some View {
        if let detail = Format.keyDetail(source: row.keySource, tier: Theme.KeyTier.of(row)) {
            detailRow("Key source", detail)
        }
        if let name = row.keyName { detailRow("Name says", name) }
        if let audio = row.keyAudio { detailRow("Audio says", audio) }
    }

    private func detailRow(_ label: String, _ value: String) -> some View {
        PropertyRow(label) {
            Text(value)
                .font(Theme.Font.rowNumeric).foregroundStyle(.secondary)
                .textSelection(.enabled)
        }
    }

    var body: some View {
        VStack(spacing: Theme.Space.s2) {
            SectionLabel("Analysis")
            textRow("BPM", text: $bpmDraft, field: .bpm, mixed: bpm.isMixed)
            keyRow
            if single { keyDetailRows }
            popupRow("Category", common: category, options: model.organizeState.categories, field: .category)
            popupRow("Kind", common: kind, options: ["loop", "one_shot"], field: .kind)
            if let error = model.organizeState.overrideError {
                Text(error)
                    .font(Theme.Font.caption).foregroundStyle(Theme.Color.error)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .accessibilityLabel("Override error: \(error)")
            }
        }
        .onChange(of: row.fileId, initial: true) { syncDrafts() }
        // Only a changed value (revert, re-analysis) resyncs; a plain row
        // refresh must not clobber an in-progress edit.
        .onChange(of: row.bpm) { syncDrafts() }
        .onChange(of: row.key) { syncDrafts() }
        .onChange(of: model.selectedFileIDs) { syncDrafts() }
        .onChange(of: bpm) { syncDrafts() }
        .onChange(of: key) { syncDrafts() }
    }

    private func syncDrafts() {
        bpmDraft = bpm.value.map { $0 == $0.rounded() ? String(Int($0)) : String($0) } ?? ""
        keyDraft = key.value ?? ""
    }

    /// Key row: the rendered label (note + superscript cents for a one-shot,
    /// else the key) until it is clicked, then the text field; Return commits,
    /// losing focus cancels. "Mixed" selections show the field directly.
    @ViewBuilder private var keyRow: some View {
        if editingKey || key.isMixed {
            textRow("Key", text: $keyDraft, field: .key, mixed: key.isMixed, focus: $keyFocused) {
                editingKey = false
            }
            .onAppear { if editingKey { Task { keyFocused = true } } }
            .onChange(of: keyFocused) { if !keyFocused { editingKey = false; syncDrafts() } }
        } else {
            PropertyRow("Key") {
                HStack(spacing: Theme.Space.s4) {
                    KeyLabelView(parts: keyParts)
                        .frame(maxWidth: .infinity, alignment: .trailing)
                        .contentShape(Rectangle())
                        .onTapGesture { editingKey = true }
                        .accessibilityAddTraits(.isButton)
                        .accessibilityAction { editingKey = true }
                        .help("Click to edit the key")
                    revert("Key", .key)
                }
            }
        }
    }

    private var keyParts: KeyLabel.Parts? {
        single
            ? KeyLabel.parts(row)
            : KeyLabel.parts(key: key.value, noteName: nil, noteCents: nil)
    }

    private func textRow(
        _ title: String, text: Binding<String>, field: OverrideField, mixed: Bool,
        focus: FocusState<Bool>.Binding? = nil, onCommit: (() -> Void)? = nil
    ) -> some View {
        PropertyRow(title) {
            HStack(spacing: Theme.Space.s4) {
                TextField(title, text: text, prompt: Text(mixed ? "Mixed" : "\u{2013}"))
                    .textFieldStyle(.plain)
                    .controlSize(.small)
                    .multilineTextAlignment(.trailing)
                    .padding(.horizontal, Theme.Space.s6)
                    .padding(.vertical, Theme.Space.s2)
                    .background(Theme.Color.surfaceWell, in: RoundedRectangle(cornerRadius: Theme.Radius.medium))
                    .modifier(OptionalFocus(focus: focus))
                    .onSubmit {
                        commit(field, text.wrappedValue)
                        onCommit?()
                    }
                revert(title, field)
            }
        }
    }

    private func popupRow(
        _ title: String, common: CommonValue<String>, options: [String], field: OverrideField
    ) -> some View {
        let current = common.value
        return PropertyRow(title) {
            HStack(spacing: Theme.Space.s4) {
                Picker(title, selection: Binding(
                    get: { current ?? "" },
                    set: { value in
                        guard !value.isEmpty, value != current else { return }
                        Task { await model.setOverride(field, value: value) }
                    })
                ) {
                    if current == nil || !options.contains(current ?? "") {
                        Text(common.isMixed ? "Mixed" : "\u{2013}").tag(current ?? "")
                    }
                    ForEach(options, id: \.self) { Text(display($0, field)).tag($0) }
                }
                .labelsHidden()
                .controlSize(.small)
                .frame(maxWidth: .infinity, alignment: .trailing)
                revert(title, field)
            }
        }
    }

    private func display(_ value: String, _ field: OverrideField) -> String {
        field == .kind ? Format.kindTitle(value) : Format.categoryTitle(value)
    }

    private func revert(_ title: String, _ field: OverrideField) -> some View {
        Button {
            Task { await model.revertOverride(field) }
        } label: {
            Image(systemName: "arrow.uturn.backward").imageScale(.small)
        }
        .buttonStyle(.borderless)
        .foregroundStyle(.secondary)
        .help("Revert to detected")
        .accessibilityLabel("Revert \(title) to detected")
    }

    private func commit(_ field: OverrideField, _ value: String) {
        let value = value.trimmingCharacters(in: .whitespaces)
        guard !value.isEmpty else { return syncDrafts() }
        Task {
            if await !model.setOverride(field, value: value) { syncDrafts() }
        }
    }
}

/// Applies `.focused` only when a binding is given.
private struct OptionalFocus: ViewModifier {
    let focus: FocusState<Bool>.Binding?
    func body(content: Content) -> some View {
        if let focus { content.focused(focus) } else { content }
    }
}
