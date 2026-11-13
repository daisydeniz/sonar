import SwiftUI

/// Inspector for the selected row's fields.
@MainActor
struct InspectorView: View {
    let row: FileRow?
    let preview: AudioPreview
    let onToggleFavorite: (FileRow) -> Void

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
                }
                PreviewControlsView(preview: preview)
                Section("Details") {
                    field("Path", row.path)
                    field("Category", Format.text(row.category))
                    field("Kind", Format.text(row.kind))
                    field("BPM", Format.bpm(row.bpm))
                    field("Key", Format.text(row.key))
                    field("Duration", Format.duration(ms: row.durationMs))
                    field("Sample rate", row.sampleRate.map { "\($0) Hz" } ?? "-")
                    field("Channels", row.channels.map(String.init) ?? "-")
                    field("Tags", row.tags.isEmpty ? "-" : row.tags.joined(separator: ", "))
                    field("Favorite", row.favorite ? "Yes" : "No")
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
