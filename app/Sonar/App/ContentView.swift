import SwiftUI

/// Main window: sidebar, toolbar search, results table and inspector.
@MainActor
struct ContentView: View {
    @Bindable var model: AppModel
    let license: LicenseManager
    @State private var showInspector = true
    @State private var searchMonitor = SearchKeyMonitor()
    @State private var tagPopoverPresenter = TagPopoverPresenter()
    @State private var dropTargeted = false
    @Environment(\.undoManager) private var undoManager

    var body: some View {
        Group {
            if license.showsGate {
                LicenseGateView(license: license)
            } else {
                mainContent
            }
        }
        .sheet(isPresented: Bindable(license).showsEnterSheet) {
            EnterLicenseView(license: license) { license.showsEnterSheet = false }
        }
        .onReceive(NotificationCenter.default.publisher(for: NSApplication.didBecomeActiveNotification)) { _ in
            Task { await license.refresh() }
        }
    }

    private var mainContent: some View {
        NavigationSplitView {
            SidebarView(model: model)
                .navigationSplitViewColumnWidth(min: 180, ideal: 220)
        } detail: {
            detail
                .navigationTitle(model.scopeTitle)
                .navigationSubtitle(model.scopeSubtitle)
                .toolbar {
                    ToolbarItem { TrialBadgeView(license: license) }
                    if #available(macOS 26, *) { ToolbarSpacer(.fixed) }
                    ToolbarItem { IndexingStatusView(model: model) }
                    // While the inspector is hidden its toolbar is gone, so the
                    // toggle moves to the trailing edge of the detail column.
                    if !showInspector {
                        if #available(macOS 26, *) { ToolbarSpacer(.flexible) }
                        ToolbarItem { inspectorToggle }
                    }
                }
        }
        .inspector(isPresented: $showInspector) {
            InspectorView(model: model, row: model.selectedRow,
                onToggleFavorite: { _ in Task { await model.toggleFavoriteOfSelection() } },
                onFindSimilar: { model.findSimilar(to: $0) })
            .inspectorColumnWidth(min: 240, ideal: 280, max: 400)
            .toolbar {
                if #available(macOS 26, *) { ToolbarSpacer(.flexible) }
                ToolbarItem { inspectorToggle }
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
        .onChange(of: model.organizeState.tagPopover) { tagPopoverPresenter.sync(model: model) }
        .sheet(isPresented: Bindable(model.organizeState).showsManageTags) { TagManageSheet(app: model) }
        .onSubmit(of: .search) { _ = model.keyboard.focusResults?() }
        .onAppear {
            searchMonitor.install(actions: model.keyboard)
            if let undoManager { model.organizeState.undoManager = undoManager }
        }
        .onDisappear { searchMonitor.remove() }
        .alert("Sonar", isPresented: errorShown) {
            Button("OK") { model.errorMessage = nil }
        } message: {
            Text(model.errorMessage ?? "")
        }
    }

    /// Shows or hides the inspector column; also View > Show Inspector (⌃⌘I).
    private var inspectorToggle: some View {
        Button { showInspector.toggle() } label: { Label("Inspector", systemImage: "sidebar.trailing") }
            .help("Show or hide the inspector (⌃⌘I)")
    }

    private var errorShown: Binding<Bool> {
        Binding(get: { model.errorMessage != nil }, set: { if !$0 { model.errorMessage = nil } })
    }

    private var detail: some View {
        VStack(spacing: 0) {
            StatusBannerView(banner: model.banner)
            detailContent
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .background(Theme.Color.surfaceContent)
        }
    }

    @ViewBuilder private var detailContent: some View {
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
                PlayerHeaderView(
                    row: model.selectedRow, preview: model.preview, waveform: model.waveform,
                    target: model.similarity.well, targetWaveform: model.targetWaveform,
                    onToggleTarget: { model.toggleTargetPlayback() },
                    onFindSimilarToSelection: { model.findSimilarToSelection() })
                if model.showsFilterBar {
                    FilterBarView(model: model)
                    Hairline()
                }
                if model.similarity.isActive {
                    SimilarityBarView(model: model)
                    Hairline()
                }
                results
            }
            .onDrop(of: [.fileURL], delegate: SimilarityDropDelegate(model: model, isTargeted: $dropTargeted))
            .overlay {
                if dropTargeted { DropOverlayView() }
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
                playingFileID: model.preview.isPlaying && !model.preview.isTargetPlaying ? model.selectedRow?.fileId : nil,
                sortKey: model.sortKey, sortDescending: model.sortDescending,
                onSelect: { model.select($0) },
                onSort: { key, descending in Task { await model.setSort(key, descending: descending) } },
                onSelectionChange: { model.selectedFileIDs = $0 },
                onKey: { key in
                    if key == .escape, model.waveform.region != nil {
                        model.waveform.clearRegion()
                        return true
                    }
                    if key == .escape, model.similarity.isActive {
                        model.exitSimilarity()
                        return true
                    }
                    return model.keyboard.handle(key, selected: model.selectedRow, selection: key == .returnKey ? model.selectedRows : [])
                },
                onFindSimilar: { model.findSimilar(to: $0) },
                onToggleFavorite: { row in Task { await model.toggleFavorite(rows: [row]) } },
                menuEntries: { model.rowMenuEntries(for: $0) },
                register: { controller in
                    model.keyboard.focusResults = { [weak controller] in controller?.focusTable() ?? false }
                    model.keyboard.popoverAnchor = { [weak controller] id in
                        controller?.popoverAnchor(forFileId: id)                    }
                })
        }
    }
}
