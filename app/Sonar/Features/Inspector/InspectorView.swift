import SwiftUI

/// Inspector for the selected row's fields.
@MainActor
struct InspectorView: View {
    let model: AppModel
    let row: FileRow?
    let preview: AudioPreview
    let waveform: WaveformModel
    let onToggleFavorite: (FileRow) -> Void
    let onFindSimilar: (FileRow) -> Void

    var body: some View {
        if let row {
            Form {
                Section {
                    Text(row.name).font(.headline).textSelection(.enabled)
                    Button {
                        onToggleFavorite(row)
                    } label: {
                        Label(row.favorite ? "Unfavorite" : "Favorite",
                              systemImage: row.favorite ? "heart.fill" : "heart")
                    }
                    .help("Favorite (⌘L)")
                    if model.selectedFileIDs.count > 1 {
                        Text("Edits apply to \(model.selectedFileIDs.count) selected samples")
                            .font(.caption).foregroundStyle(.secondary)
                    }
                    Button { onFindSimilar(row) } label: { Label("Find Similar", systemImage: "waveform.badge.magnifyingglass") }
                }
                Section("Waveform") { WaveformView(model: waveform, preview: preview) }
                PreviewControlsView(preview: preview)
                OverrideFieldsView(model: model, row: row)
                TagsEditorView(model: model, row: row)
                Section("Details") {
                    field("Path", row.path)
                    field("Duration", Format.duration(ms: row.durationMs))
                    field("Sample rate", row.sampleRate.map { "\($0) Hz" } ?? "-")
                    field("Channels", row.channels.map(String.init) ?? "-")
                    field("Favorite", row.favorite ? "Yes" : "No")
                    if row.hidden { field("Hidden", "Yes") }
                }
            }
            .formStyle(.grouped)
        } else {
            Text("No selection")
                .foregroundStyle(.secondary)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }

    private func field(_ title: String, _ value: String) -> some View {
        LabeledContent(title) {
            Text(value).textSelection(.enabled).multilineTextAlignment(.trailing)
        }
        .accessibilityElement(children: .combine)
    }
}
