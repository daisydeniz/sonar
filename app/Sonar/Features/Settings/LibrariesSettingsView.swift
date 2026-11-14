import SwiftUI

/// Settings tab: libraries with online/offline badge, add / rescan / remove
/// (with confirmation) / reveal.
@MainActor
struct LibrariesSettingsView: View {
    let model: AppModel
    @State private var pendingRemoval: LibraryInfo?
    @State private var selection: Int64?

    var body: some View {
        VStack(spacing: 8) {
            if model.libraries.isEmpty {
                EmptyStateView(
                    symbol: "folder.badge.plus", title: "No libraries",
                    message: "Add a folder of samples to start.")
            } else {
                List(model.libraries, id: \.id, selection: $selection) { library in
                    HStack {
                        Text(library.rootPath).lineLimit(1).truncationMode(.middle)
                        Spacer()
                        Text(library.online ? "Online" : "Offline")
                            .font(.caption.bold())
                            .padding(.horizontal, 6).padding(.vertical, 1)
                            .background((library.online ? Color.green : Color.orange).opacity(0.2), in: Capsule())
                    }
                    .tag(library.id)
                    .accessibilityElement(children: .combine)
                    .accessibilityLabel("\(library.rootPath), \(library.online ? "online" : "offline")")
                }
            }
            HStack {
                Button("Add…") {
                    let paths = FolderPicker.pickFolders()
                    if !paths.isEmpty { Task { await model.addLibraries(paths: paths) } }
                }
                Spacer()
                Button("Reveal in Finder") {
                    if let library = chosen {
                        NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: library.rootPath)])
                    }
                }
                .disabled(chosen == nil)
                Button("Rescan") {
                    if let library = chosen { Task { await model.rescanLibrary(id: library.id) } }
                }
                .disabled(chosen?.online != true)
                Button("Remove…", role: .destructive) { pendingRemoval = chosen }
                    .disabled(chosen == nil)
            }
            .padding(.horizontal)
        }
        .padding(.vertical)
        .confirmationDialog(
            "Remove this library from Sonar?",
            isPresented: Binding(get: { pendingRemoval != nil }, set: { if !$0 { pendingRemoval = nil } }),
            presenting: pendingRemoval
        ) { library in
            Button("Remove", role: .destructive) { Task { await model.removeLibrary(id: library.id) } }
        } message: { library in
            Text("\(library.rootPath)\nYour files on disk are not touched. Sonar's analysis data for them is dropped.")
        }
    }

    private var chosen: LibraryInfo? { model.libraries.first { $0.id == selection } }
}
