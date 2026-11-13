import SwiftUI

/// Inspector fields for BPM / Key / Category / Kind. Editing writes a user
/// override for the whole selection (the core keeps it across re-analysis);
/// "Revert to detected" clears it. A value the core rejects shows its message
/// inline and the field reverts.
@MainActor
struct OverrideFieldsView: View {
    @Bindable var model: AppModel
    let row: FileRow
    @State private var bpmDraft = ""
    @State private var keyDraft = ""

    var body: some View {
        Section("Detected values") {
            textRow("BPM", text: $bpmDraft, field: .bpm)
            textRow("Key", text: $keyDraft, field: .key)
            popupRow("Category", current: row.category, options: model.organizeState.categories, field: .category)
            popupRow("Kind", current: row.kind, options: ["loop", "one_shot"], field: .kind)
            if let error = model.organizeState.overrideError {
                Text(error).font(.caption).foregroundStyle(.red)
                    .accessibilityLabel("Override error: \(error)")
            }
        }
        .onChange(of: row.fileId, initial: true) { syncDrafts() }
        // Only a changed value (revert, re-analysis) resyncs; a plain row
        // refresh must not clobber an in-progress edit.
        .onChange(of: row.bpm) { syncDrafts() }
        .onChange(of: row.key) { syncDrafts() }
    }

    private func syncDrafts() {
        bpmDraft = row.bpm.map { $0 == $0.rounded() ? String(Int($0)) : String($0) } ?? ""
        keyDraft = row.key ?? ""
    }

    private func textRow(_ title: String, text: Binding<String>, field: OverrideField) -> some View {
        LabeledContent(title) {
            HStack {
                TextField(title, text: text)
                    .multilineTextAlignment(.trailing)
                    .onSubmit { commit(field, text.wrappedValue) }
                revert(title, field)
            }
        }
    }

    private func popupRow(_ title: String, current: String?, options: [String], field: OverrideField) -> some View {
        LabeledContent(title) {
            HStack {
                Picker(title, selection: Binding(
                    get: { current ?? "" },
                    set: { value in
                        guard !value.isEmpty, value != current else { return }
                        Task { await model.setOverride(field, value: value) }
                    })
                ) {
                    if current == nil || !options.contains(current ?? "") { Text("-").tag(current ?? "") }
                    ForEach(options, id: \.self) { Text($0).tag($0) }
                }
                .labelsHidden()
                revert(title, field)
            }
        }
    }

    private func revert(_ title: String, _ field: OverrideField) -> some View {
        Button {
            Task { await model.revertOverride(field) }
        } label: {
            Image(systemName: "arrow.uturn.backward")
        }
        .buttonStyle(.borderless)
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
