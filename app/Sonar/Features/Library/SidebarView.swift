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
            }
            Section("Libraries") {
                ForEach(model.libraries, id: \.id) { library in
                    Label(Self.title(library), systemImage: library.online ? "folder" : "folder.badge.questionmark")
                        .tag(SidebarItem.library(library.id))
                        .help(library.rootPath)
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
                }
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
        if let path = FolderPicker.pickFolder() {
            Task { await model.addLibrary(path: path) }
        }
    }

    private static func title(_ library: LibraryInfo) -> String {
        URL(fileURLWithPath: library.rootPath).lastPathComponent
    }
}
