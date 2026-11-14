import SwiftUI

/// Main window: sidebar, toolbar search, results table and inspector.
@MainActor
struct ContentView: View {
    @Bindable var model: AppModel
    @State private var showInspector = true
    @State private var searchMonitor = SearchKeyMonitor()
    @State private var dropTargeted = false

    var body: some View {
        NavigationSplitView {
            SidebarView(model: model)
                .navigationSplitViewColumnWidth(min: 180, ideal: 220)
        } detail: {
            detail
                .inspector(isPresented: $showInspector) {
                    InspectorView(model: model, row: model.selectedRow, preview: model.preview, waveform: model.waveform,
                        onToggleFavorite: { _ in Task { await model.toggleFavoriteOfSelection() } },
                        onFindSimilar: { model.findSimilar(to: $0) })
                    .inspectorColumnWidth(min: 240, ideal: 280, max: 400)
                }
        }
        .searchable(text: $model.searchText, prompt: "Search samples")
        .onChange(of: model.searchText) { model.searchTextChanged() }
        .onChange(of: model.sidebarSelection) { model.scheduleRefresh() }
        .onChange(of: model.filters) { model.scheduleRefresh() }
        .onChange(of: model.organizeState.showHidden) { model.scheduleRefresh() }
        .alert(
            model.organizeState.prompt?.title ?? "",
            isPresented: Binding(
                get: { model.organizeState.prompt != nil },
                set: { if !$0 { model.organizeState.prompt = nil } })
        ) {
            TextField("Name", text: Bindable(model.organizeState).promptText)
            Button(model.organizeState.prompt?.confirmTitle ?? "OK") { Task { await model.commitPrompt() } }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text(model.organizeState.prompt?.message ?? "")
        }
        .onSubmit(of: .search) { _ = model.keyboard.focusResults?() }
        .onAppear { searchMonitor.install(actions: model.keyboard) }
        .onDisappear { searchMonitor.remove() }
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
            EngineFailedView(model: model, message: message)
        case .ready where model.showsOnboarding:
            OnboardingView(model: model)
        case .ready where model.sidebarSelection == .duplicates:
            DuplicatesView(model: model)
        case .ready:
            VStack(spacing: 0) {
                StatusBannerView(banner: model.banner)
                if model.showsFilterBar {
                    FilterBarView(model: model)
                    Divider()
                }
                if model.similarity.isActive {
                    SimilarityBarView(model: model)
                    Divider()
                }
                results
            }
            .onDrop(of: [.fileURL], delegate: SimilarityDropDelegate(model: model, isTargeted: $dropTargeted))
            .overlay {
                if dropTargeted {
                    RoundedRectangle(cornerRadius: 6).stroke(Color.accentColor, lineWidth: 3).padding(2)
                        .allowsHitTesting(false)
                }
            }
        }
    }

    @ViewBuilder private var results: some View {
        if model.resultCount == 0, model.similarity.isActive, model.similarity.isLoading {
            ProgressView("Finding similar samples…").frame(maxWidth: .infinity, maxHeight: .infinity)
        } else if let kind = model.emptyState {
            ResultsEmptyStateView(model: model, kind: kind)
        } else {
            ResultsTableView(
                pager: model.pager,
                libraryNames: Dictionary(uniqueKeysWithValues: model.libraries.map { ($0.id, URL(fileURLWithPath: $0.rootPath).lastPathComponent) }),
                onSelect: { model.select($0) },
                onSort: { key, descending in Task { await model.setSort(key, descending: descending) } },
                onSelectionChange: { model.selectedFileIDs = $0 },
                onKey: { key in
                    if key == .escape, model.similarity.isActive {
                        model.exitSimilarity()
                        return true
                    }
                    return model.keyboard.handle(key, selected: model.selectedRow, selection: key == .returnKey ? model.selectedRows : [])
                },
                onFindSimilar: { model.findSimilar(to: $0) },
                menuEntries: { model.rowMenuEntries(for: $0) },
                register: { controller in
                    model.keyboard.focusResults = { [weak controller] in controller?.focusTable() ?? false }
                })
        }
    }
}
