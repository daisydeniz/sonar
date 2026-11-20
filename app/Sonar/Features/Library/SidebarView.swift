import SwiftUI

/// Sidebar: Library (All Samples, Favorites), Libraries, Collections
/// (DESIGN §3). Standard `.sidebar` list; counts are badges, offline
/// libraries are dimmed with a warning symbol, "Add…" is a `+` button in the
/// section header.
@MainActor
struct SidebarView: View {
    @Bindable var model: AppModel
    @State private var pendingRemoval: LibraryInfo?

    var body: some View {
        List(selection: $model.sidebarSelection) {
            Section("Library") {
                Label("All Samples", systemImage: "waveform").tag(SidebarItem.allSamples)
                Label("Favorites", systemImage: "heart")
                    .badge(Int(model.facets?.favorites ?? 0))
                    .tag(SidebarItem.favorites)
                Label("Duplicates", systemImage: "square.on.square").tag(SidebarItem.duplicates)
            }
            Section {
                ForEach(model.libraries, id: \.id) { library in
                    libraryRow(library)
                }
            } header: {
                SidebarSectionHeader(title: "Libraries", addLabel: "Add Library", action: addLibrary)
            }
            Section {
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
            } header: {
                SidebarSectionHeader(title: "Collections", addLabel: "New Collection") {
                    model.organizeState.ask(.newCollection([]))
                }
            }
            SidebarTagsSection(model: model)
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

    @ViewBuilder private func libraryRow(_ library: LibraryInfo) -> some View {
        let title = Self.title(library)
        let count = model.facets?.libraries.first { $0.libraryId == library.id }?.count
        Group {
            if library.online {
                Label(title, systemImage: "folder")
            } else {
                Label {
                    Text(title).foregroundStyle(.tertiary)
                } icon: {
                    Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(Theme.Color.warning)
                }
            }
        }
        .badge(library.online ? Int(count ?? 0) : 0)
        .tag(SidebarItem.library(library.id))
        .help(library.online ? library.rootPath : "Offline: \(library.rootPath) is not available")
        .accessibilityLabel(library.online ? title : "\(title), offline")
        .contextMenu {
            Button("Remove Library…", role: .destructive) { pendingRemoval = library }
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

/// Sidebar section header with a trailing `+` button (instead of a fake row).
private struct SidebarSectionHeader: View {
    let title: String
    let addLabel: String
    let action: () -> Void

    var body: some View {
        HStack {
            Text(title)
            Spacer()
            Button(action: action) { Image(systemName: "plus") }
                .buttonStyle(.borderless)
                .controlSize(.small)
                .help("\(addLabel)…")
                .accessibilityLabel(addLabel)
        }
    }
}
