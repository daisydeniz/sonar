import AppKit
import SwiftUI
import XCTest
@testable import Sonar

/// Renders the main screens with deterministic data to PNGs (light + dark)
/// so agents can look at the UI (`scripts/ui-snapshots.sh`, DESIGN §6).
/// Skipped unless `SONAR_UI_SNAPSHOT_DIR` is set.
///
/// Rendering: the SwiftUI view is hosted in an `NSHostingView` inside a
/// borderless window placed off screen and captured through the window
/// server (`cacheDisplay` is only the fallback). Limitations: the window is
/// never key, so focus-dependent styles (prominent buttons) show their
/// inactive look; the table's selection style is forced through
/// `ResultsRowView.emphasisOverride` (focused for `main`, inactive for
/// `main-unfocused`) instead of stealing keyboard focus; no real title bar or toolbar (the
/// window is borderless), so toolbar items and the search field are not in
/// the pictures; sidebar, filter bar, table, inspector and banners are.
@MainActor
final class UISnapshotTests: XCTestCase {
    private static let size = CGSize(width: 1280, height: 760)

    private func outputDir() throws -> URL {
        guard let dir = ProcessInfo.processInfo.environment["SONAR_UI_SNAPSHOT_DIR"], !dir.isEmpty else {
            throw XCTSkip("SONAR_UI_SNAPSHOT_DIR not set (scripts/ui-snapshots.sh sets it)")
        }
        let url = URL(fileURLWithPath: dir)
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        return url
    }

    private func makeModel() -> AppModel {
        let name = "ui-snapshots-\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: name)!
        addTeardownBlock { UserDefaults().removePersistentDomain(forName: name) }
        defaults.set(false, forKey: AudioPreview.autoPlayKey)
        let model = AppModel(
            preview: AudioPreview(player: FakePlayer(), defaults: defaults), settings: AppSettings(defaults: defaults))
        model.waveform.provider = SnapshotWaveformProvider()
        return model
    }

    /// Model with libraries, collections, rows and the third row selected.
    private func populated(rows: [FileRow] = SnapshotFixtures.rows()) async throws -> AppModel {
        let model = makeModel()
        model.installStaticState(libraries: SnapshotFixtures.libraries, collections: SnapshotFixtures.collections)
        _ = try await model.pager.reset(loader: SnapshotPageLoader(rows: rows))
        model.select(rows.count > 2 ? rows[2] : nil)
        await model.waveform.settle()
        return model
    }

    private func contentView(_ model: AppModel) -> some View {
        ContentView(model: model, license: LicenseManager(service: SnapshotLicenseService()))
    }

    func testMain() async throws {
        let dir = try outputDir()
        let model = try await populated()
        try snapshot("main", contentView(model), dir: dir, emphasis: true, selectRow: 2)
    }

    /// Filter bar with active chips (category, BPM, tags) and favorites on.
    func testMainFilters() async throws {
        let dir = try outputDir()
        let model = try await populated()
        model.filters.categories = ["kick", "snare"]
        model.filters.kinds = ["loop"]
        model.filters.bpmMin = 90
        model.filters.bpmMax = 130
        model.filters.tags = ["dark"]
        model.filters.favoritesOnly = true
        try snapshot("main-filters", contentView(model), dir: dir, emphasis: true, selectRow: 2)
    }

    /// An open filter popover. A real popover is its own window and is not in
    /// the captured window, so its content is drawn on a card under the chip.
    func testMainFilterPopover() async throws {
        let dir = try outputDir()
        let model = try await populated()
        model.filters.categories = ["bass", "clap"]
        let chips = FacetChip.chips(facets: model.facets?.categories ?? [], selected: model.filters.categories)
        let card = FacetListView(
            title: "Category", chips: chips, display: Format.categoryTitle, dot: { Theme.categoryColor($0) }
        ) { _ in }
        .padding(Theme.Space.s12)
        .frame(width: Theme.Size.popoverWidth)
        .background(Theme.Color.surfaceRaised, in: RoundedRectangle(cornerRadius: Theme.Radius.large))
        .overlay {
            RoundedRectangle(cornerRadius: Theme.Radius.large)
                .strokeBorder(Theme.Color.hairline, lineWidth: Theme.hairlineWidth)
        }
        let view = contentView(model).overlay(alignment: .topLeading) {
            card.padding(.top, 260).padding(.leading, 400)
        }
        try snapshot("main-filter-popover", view, dir: dir, emphasis: true, selectRow: 2)
    }

    /// Player header without a selection: disabled transport, flat centre line.
    func testMainNoSelection() async throws {
        let dir = try outputDir()
        let model = try await populated()
        model.select(nil)
        await model.waveform.settle()
        try snapshot("main-noselection", contentView(model), dir: dir, emphasis: true)
    }

    /// Inspector with several samples selected: count header, batch actions.
    func testMainMultiSelection() async throws {
        let dir = try outputDir()
        let rows = SnapshotFixtures.rows()
        let model = try await populated(rows: rows)
        try snapshot("main-multi", contentView(model), dir: dir, emphasis: true, selectRows: IndexSet(2...4))
    }

    /// Waveform with a selected region: tint, edge handles, readout and menu.
    func testMainRegion() async throws {
        let dir = try outputDir()
        let model = try await populated()
        let duration = try XCTUnwrap(model.waveform.duration)
        model.waveform.setRegion(WaveformRegion(from: 0.12, to: 0.4, duration: duration))
        try snapshot("main-region", contentView(model), dir: dir, emphasis: true, selectRow: 2)
    }

    func testMainUnfocused() async throws {
        let dir = try outputDir()
        let model = try await populated()
        try snapshot("main-unfocused", contentView(model), dir: dir, emphasis: false, selectRow: 2)
    }

    /// Table with rows whose page is not loaded: skeleton bars in Name.
    func testTableLoading() async throws {
        let dir = try outputDir()
        let pager = ResultsPager(pageSize: 20)
        _ = try await pager.reset(loader: StalledPageLoader(rows: SnapshotFixtures.rows()))
        let table = ResultsTableView(
            pager: pager, libraryNames: [1: "KSHMR Vol 1", 2: "Drum Hits"], sortKey: .relevance, sortDescending: false,
            onSelect: { _ in }, onSort: { _, _ in }, onSelectionChange: { _ in }, onKey: { _ in false },
            onFindSimilar: { _ in }, onToggleFavorite: { _ in }, register: { _ in })
        try snapshot("table-loading", table, dir: dir, emphasis: true, selectRow: 1)
    }

    func testMainSimilar() async throws {
        let dir = try outputDir()
        let rows = SnapshotFixtures.rows()
        let model = try await populated(rows: rows)
        let seed = rows[2]
        model.targetWaveform.provider = SnapshotWaveformProvider()
        // Compare mode: the target well shows the seed with its region.
        model.similarity.begin(
            .file(id: seed.fileId, name: seed.name), region: SimilarRegion(startS: 0.05, endS: 0.2), row: seed)
        model.syncTargetWaveform()
        await model.targetWaveform.settle()
        let neighbours = Array(rows.dropFirst(10).prefix(60))
        _ = try await model.pager.reset(loader: ArrayResultLoader(rows: neighbours))
        model.similarity.finish(
            hits: neighbours.enumerated().map {
                SimilarHit(
                    fileId: $1.fileId, distance: Float($0) * 0.01, spectrum: 0.1, timbre: 0.1, pitch: 0.1,
                    amplitude: 0.1)
            })
        try snapshot("main-similar", contentView(model), dir: dir)
    }

    /// The overlay shown while an audio file is dragged over the results.
    func testMainDrop() async throws {
        let dir = try outputDir()
        let model = try await populated()
        // The real overlay covers the detail column: between sidebar (220) and inspector (280).
        let view = contentView(model).overlay {
            DropOverlayView().padding(.leading, 220).padding(.trailing, 280)
        }
        try snapshot("main-drop", view, dir: dir, emphasis: true, selectRow: 2)
    }

    func testEmpty() async throws {
        let dir = try outputDir()
        let model = try await populated(rows: [])
        model.searchText = "zzzzz"
        try snapshot("empty", contentView(model), dir: dir)
    }

    func testOnboarding() async throws {
        let dir = try outputDir()
        let model = makeModel()
        model.installStaticState(libraries: [], collections: [])
        try snapshot("onboarding", contentView(model), dir: dir)
    }

    func testDuplicates() async throws {
        let dir = try outputDir()
        let rows = SnapshotFixtures.rows()
        let model = try await populated(rows: rows)
        model.duplicates.apply(SnapshotFixtures.duplicates(rows: rows), rows: rows)
        model.sidebarSelection = .duplicates
        try snapshot("duplicates", contentView(model), dir: dir)
    }

    func testComponents() async throws {
        let dir = try outputDir()
        try snapshot("components", ComponentsGallery(), dir: dir)
    }

    // MARK: Rendering

    private func snapshot<V: View>(
        _ name: String, _ view: V, dir: URL, emphasis: Bool? = nil, selectRow: Int? = nil,
        selectRows: IndexSet? = nil
    ) throws {
        let selection = selectRows ?? selectRow.map { IndexSet(integer: $0) }
        ResultsRowView.emphasisOverride = emphasis
        defer { ResultsRowView.emphasisOverride = nil }
        for (suffix, appearanceName) in [("light", NSAppearance.Name.aqua), ("dark", .darkAqua)] {
            let appearance = try XCTUnwrap(NSAppearance(named: appearanceName))
            let host = NSHostingView(rootView: view.frame(width: Self.size.width, height: Self.size.height))
            let window = NSWindow(
                contentRect: NSRect(origin: NSPoint(x: -6000, y: -6000), size: Self.size),
                styleMask: [.borderless], backing: .buffered, defer: false)
            window.appearance = appearance
            window.contentView = host
            window.isReleasedWhenClosed = false
            window.orderFrontRegardless()
            defer { window.close() }
            // Let SwiftUI lay out and the table load its first page.
            for _ in 0..<12 {
                RunLoop.main.run(until: Date().addingTimeInterval(0.1))
                host.layoutSubtreeIfNeeded()
            }
            if let selection, let table = Self.findTable(in: host) {
                table.selectRowIndexes(selection, byExtendingSelection: false)
                for _ in 0..<3 {
                    RunLoop.main.run(until: Date().addingTimeInterval(0.1))
                    host.layoutSubtreeIfNeeded()
                }
            }
            let png = try XCTUnwrap(Self.capture(window: window, host: host))
            try png.write(to: dir.appendingPathComponent("\(name)-\(suffix).png"))
        }
    }

    /// The results table inside a hosted SwiftUI view, if any.
    private static func findTable(in view: NSView) -> KeyTableView? {
        if let table = view as? KeyTableView { return table }
        for sub in view.subviews { if let found = findTable(in: sub) { return found } }
        return nil
    }

    /// Window-server capture of our own window (no Screen Recording
    /// permission needed for own windows). It includes layer-backed system
    /// controls that `cacheDisplay` renders black. `CGWindowListCreateImage`
    /// is unavailable in the new SDKs, so it is looked up at run time; if it
    /// is missing or fails, falls back to `cacheDisplay` of the view.
    private static func capture(window: NSWindow, host: NSView) -> Data? {
        typealias Fn = @convention(c) (CGRect, UInt32, UInt32, UInt32) -> Unmanaged<CGImage>?
        if let sym = dlsym(UnsafeMutableRawPointer(bitPattern: -2), "CGWindowListCreateImage") {  // RTLD_DEFAULT
            let fn = unsafeBitCast(sym, to: Fn.self)
            let includingWindow: UInt32 = 1 << 3
            let ignoreFraming: UInt32 = 1 << 0, bestResolution: UInt32 = 1 << 3
            if let image = fn(.null, includingWindow, UInt32(window.windowNumber), ignoreFraming | bestResolution)?
                .takeRetainedValue(), image.width > 0
            {
                return NSBitmapImageRep(cgImage: image).representation(using: .png, properties: [:])
            }
        }
        let bounds = host.bounds
        guard let rep = host.bitmapImageRepForCachingDisplay(in: bounds) else { return nil }
        host.cacheDisplay(in: bounds, to: rep)
        return rep.representation(using: .png, properties: [:])
    }
}
