import SwiftUI

/// Sidebar: Library (All Samples, Favorites), Libraries, Collections.
@MainActor
struct SidebarView: View {
    @Bindable var model: AppModel
    @State private var pendingRemoval: LibraryInfo?

    var body: some View {
        List(selection: $model.sidebarSelection) {
            Section("Library") {
                Label("All Samples", systemImage: "waveform").tag(SidebarItem.allSamples)
                Label("Favorites", systemImage: "heart").tag(SidebarItem.favorites)
                Label("Duplicates", systemImage: "square.on.square").tag(SidebarItem.duplicates)
            }
            Section("Libraries") {
                ForEach(model.libraries, id: \.id) { library in
                    Label(Self.title(library), systemImage: library.online ? "folder" : "externaldrive.badge.exclamationmark")
                        .opacity(library.online ? 1 : 0.5)
                        .tag(SidebarItem.library(library.id))
                        .help(library.online ? library.rootPath : "Offline: \(library.rootPath) is not available")
                        .accessibilityLabel(library.online ? Self.title(library) : "\(Self.title(library)), offline")
                        .contextMenu {
                            Button("Remove Library…", role: .destructive) { pendingRemoval = library }
                        }
                }
                Button {
                    addLibrary()
                } label: {
                    Label("Add Library…", systemImage: "plus")
                }
                .buttonStyle(.plain)
                .foregroundStyle(.secondary)
                .accessibilityLabel("Add Library")
            }
            Section("Collections") {
                ForEach(model.collections, id: \.id) { collection in
                    Label(collection.name, systemImage: "square.stack")
                        .badge(Int(collection.files))
                        .tag(SidebarItem.collection(collection.id))
                        .contextMenu {
                            Button("Rename…") {
                                model.organizeState.ask(.renameCollection(collection.id), text: collection.name)
                            }
                            Button("Delete Collection", role: .destructive) {
                                Task { await model.deleteCollection(id: collection.id) }
                            }
                        }
                }
                Button {
                    model.organizeState.ask(.newCollection([]))
                } label: {
                    Label("New Collection…", systemImage: "plus")
                }
                .buttonStyle(.plain)
                .foregroundStyle(.secondary)
                .accessibilityLabel("New Collection")
            }
        }
        .listStyle(.sidebar)
        .confirmationDialog(
            "Remove this library from Sonar?",
            isPresented: Binding(get: { pendingRemoval != nil }, set: { if !$0 { pendingRemoval = nil } }),
            presenting: pendingRemoval
        ) { library in
            Button("Remove \(Self.title(library))", role: .destructive) {
                Task { await model.removeLibrary(id: library.id) }
            }
        } message: { _ in
            Text("The files stay on disk; only Sonar's index of them is removed.")
        }
    }

    private func addLibrary() {
        let paths = FolderPicker.pickFolders()
        if !paths.isEmpty { Task { await model.addLibraries(paths: paths) } }
    }

    private static func title(_ library: LibraryInfo) -> String {
        URL(fileURLWithPath: library.rootPath).lastPathComponent
    }
}
