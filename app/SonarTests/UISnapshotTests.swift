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
/// never key, so focus-dependent styles (focused selection, prominent
/// buttons) show their inactive look; no real title bar or toolbar (the
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
        try snapshot("main", contentView(model), dir: dir)
    }

    func testMainSimilar() async throws {
        let dir = try outputDir()
        let rows = SnapshotFixtures.rows()
        let model = try await populated(rows: rows)
        let seed = rows[2]
        model.similarity.begin(.file(id: seed.fileId, name: seed.name))
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

    private func snapshot<V: View>(_ name: String, _ view: V, dir: URL) throws {
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
            let png = try XCTUnwrap(Self.capture(window: window, host: host))
            try png.write(to: dir.appendingPathComponent("\(name)-\(suffix).png"))
        }
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
