import SwiftUI

/// Main window: sidebar, toolbar search, results table and inspector.
@MainActor
struct ContentView: View {
    @Bindable var model: AppModel
    @State private var showInspector = true

    var body: some View {
        NavigationSplitView {
            SidebarView(model: model)
                .navigationSplitViewColumnWidth(min: 180, ideal: 220)
        } detail: {
            detail
                .inspector(isPresented: $showInspector) {
                    InspectorView(row: model.selectedRow) { row in
                        Task { await model.setFavorite(fileIds: [row.fileId], on: !row.favorite) }
                    }
                    .inspectorColumnWidth(min: 240, ideal: 280, max: 400)
                }
        }
        .searchable(text: $model.searchText, prompt: "Search samples")
        .onChange(of: model.searchText) { model.scheduleRefresh() }
        .onChange(of: model.sidebarSelection) { model.scheduleRefresh() }
        .toolbar {
            ToolbarItem { IndexingStatusView(model: model) }
            ToolbarItem {
                Button { showInspector.toggle() } label: { Label("Inspector", systemImage: "sidebar.trailing") }
            }
        }
        .alert("Sonar", isPresented: Binding(
            get: { model.errorMessage != nil }, set: { if !$0 { model.errorMessage = nil } })
        ) {
            Button("OK") { model.errorMessage = nil }
        } message: {
            Text(model.errorMessage ?? "")
        }
    }

    @ViewBuilder private var detail: some View {
        switch model.engineState {
        case .opening:
            ProgressView("Opening library…").frame(maxWidth: .infinity, maxHeight: .infinity)
        case .failed(let message):
            ContentUnavailableView(
                "Could not open Sonar's library", systemImage: "exclamationmark.triangle",
                description: Text(message))
        case .ready:
            if model.resultCount == 0 {
                ContentUnavailableView(
                    model.libraries.isEmpty ? "No libraries yet" : "No results",
                    systemImage: model.libraries.isEmpty ? "folder.badge.plus" : "magnifyingglass",
                    description: Text(model.libraries.isEmpty
                        ? "Add a folder of samples with File > Add Library…"
                        : "Try a different search."))
            } else {
                ResultsTableView(
                    pager: model.pager,
                    libraryNames: Dictionary(uniqueKeysWithValues: model.libraries.map { ($0.id, URL(fileURLWithPath: $0.rootPath).lastPathComponent) }),
                    onSelect: { model.select($0) },
                    onSort: { key, descending in Task { await model.setSort(key, descending: descending) } })
            }
        }
    }
}
