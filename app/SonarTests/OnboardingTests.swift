import XCTest
@testable import Sonar

/// Opener that fails the first `failures` times, then opens an engine on a
/// temp home.
private final class FlakyOpener: @unchecked Sendable {
    private let lock = NSLock()
    private var remaining: Int
    private let home: String
    init(failures: Int, home: String) { remaining = failures; self.home = home }

    func open() async throws -> CoreBridge {
        let fail = lock.withLock { () -> Bool in
            defer { remaining -= 1 }
            return remaining > 0
        }
        if fail { throw FfiError.Io(message: "cannot open the store") }
        let home = home
        return CoreBridge(engine: try await Task.detached { try SonarEngine.open(home: home) }.value)
    }
}

/// T7.2: onboarding, error states, empty states, banner, settings.
@MainActor
final class OnboardingTests: XCTestCase {
    private struct FixturesMissing: Error {}

    private func makeHome() -> String {
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("sonar-onboarding-test-\(UUID().uuidString)")
        addTeardownBlock { try? FileManager.default.removeItem(at: url) }
        return url.path
    }

    private func suite() -> UserDefaults {
        let name = "onboarding-tests-\(UUID().uuidString)"
        addTeardownBlock { UserDefaults().removePersistentDomain(forName: name) }
        return UserDefaults(suiteName: name)!
    }

    private func fixturesDir() throws -> String {
        let env = ProcessInfo.processInfo.environment
        guard let dir = env["SONAR_FIXTURES_DIR"], !dir.isEmpty else {
            if env["SONAR_REQUIRE_FIXTURES"] == "1" { throw FixturesMissing() }
            throw XCTSkip("SONAR_FIXTURES_DIR not set (scripts/check.sh sets it)")
        }
        return dir
    }

    private func makeModel(home: String, defaults: UserDefaults? = nil) async throws -> AppModel {
        let d = defaults ?? suite()
        let engine = try await Task.detached { try SonarEngine.open(home: home) }.value
        return AppModel(
            engine: engine, preview: AudioPreview(player: FakePlayer(), defaults: d),
            activePollMs: 20, idlePollMs: 50, settings: AppSettings(defaults: d))
    }

    // MARK: Onboarding

    func testOnboardingShownWithZeroLibrariesAndHiddenAfterAdding() async throws {
        let fixtures = try fixturesDir()
        let model = try await makeModel(home: makeHome())
        defer { model.stop() }
        XCTAssertFalse(model.showsOnboarding, "not before the sidebar loaded")
        await model.start()
        XCTAssertTrue(model.showsOnboarding)
        XCTAssertNil(model.emptyState, "onboarding replaces the empty table")
        await model.addLibraries(paths: [fixtures])
        XCTAssertFalse(model.showsOnboarding)
        XCTAssertEqual(model.libraries.count, 1)
    }

    // MARK: Engine open failure

    func testEngineOpenFailureThenRetrySucceeds() async throws {
        let opener = FlakyOpener(failures: 1, home: makeHome())
        let d = suite()
        let model = AppModel(
            preview: AudioPreview(player: FakePlayer(), defaults: d), activePollMs: 20, idlePollMs: 50,
            settings: AppSettings(defaults: d), opener: { try await opener.open() })
        defer { model.stop() }
        await model.start()
        XCTAssertEqual(model.engineState, .failed("cannot open the store"))
        XCTAssertFalse(model.showsOnboarding)
        await model.retryOpen()
        XCTAssertEqual(model.engineState, .ready)
        XCTAssertTrue(model.showsOnboarding, "normal state: no libraries yet")
    }

    // MARK: Empty states

    func testNoResultsEmptyStateOnlyWhenNotLoading() async throws {
        let fixtures = try fixturesDir()
        let model = try await makeModel(home: makeHome())
        defer { model.stop() }
        XCTAssertTrue(model.isLoadingResults, "nothing loaded yet")
        XCTAssertNil(model.emptyState)
        await model.start()
        await model.addLibrary(path: fixtures)
        XCTAssertNil(model.emptyState, "rows are shown")

        model.searchText = "zzzqqqxxx"
        await model.refreshResults(keepScroll: false)
        XCTAssertEqual(model.resultCount, 0)
        XCTAssertFalse(model.isLoadingResults)
        XCTAssertEqual(model.emptyState, .noResults)

        model.clearSearchAndFilters()
        XCTAssertEqual(model.searchText, "")
        XCTAssertFalse(model.filters.isActive)
        await model.refreshResults(keepScroll: false)
        XCTAssertNil(model.emptyState)

        model.sidebarSelection = .favorites
        await model.refreshResults(keepScroll: false)
        XCTAssertEqual(model.emptyState, .noFavorites)
    }

    // MARK: Banner

    func testRepeatedErrorEventsCoalesceIntoOneBanner() throws {
        let d = suite()
        let model = AppModel(preview: AudioPreview(player: FakePlayer(), defaults: d), settings: AppSettings(defaults: d))
        for _ in 0..<3 { XCTAssertFalse(model.apply(.error(message: "watcher failed"))) }
        XCTAssertEqual(model.banner.message, "watcher failed")
        XCTAssertEqual(model.banner.count, 3)
        XCTAssertEqual(model.banner.displayText, "watcher failed (3 times)")
        XCTAssertNil(model.errorMessage, "no modal alert for events")
        _ = model.apply(.error(message: "disk full"))
        XCTAssertEqual(model.banner.displayText, "disk full")
        XCTAssertTrue(model.apply(.libraryOffline(libraryId: 1)))
        model.banner.dismiss()
        XCTAssertNil(model.banner.displayText)
    }

    // MARK: Settings

    func testSettingsPersistAndPreviewReadsThem() async throws {
        let d = suite()
        let player = FakePlayer()
        let preview = AudioPreview(player: player, decoder: FakeDecoder(), defaults: d)
        XCTAssertTrue(preview.autoPlay)
        XCTAssertTrue(preview.normalizeLoudness)
        preview.select(previewRow(1, gain: -3.5))
        await preview.settle()
        XCTAssertEqual(player.plays.last?.gainDb, -3.5)

        preview.normalizeLoudness = false
        preview.autoPlay = false
        let settings = AppSettings(defaults: d)
        XCTAssertTrue(settings.includeEmbeddings)
        settings.includeEmbeddings = false

        // A fresh instance on the same suite sees the saved values.
        let player2 = FakePlayer()
        let again = AudioPreview(player: player2, decoder: FakeDecoder(), defaults: d)
        XCTAssertFalse(again.autoPlay)
        XCTAssertFalse(again.normalizeLoudness)
        XCTAssertFalse(AppSettings(defaults: d).includeEmbeddings)
        again.select(previewRow(2, gain: -3.5))
        again.toggle()
        await again.settle()
        XCTAssertNotNil(player2.plays.last)
        XCTAssertNil(player2.plays.last?.gainDb, "normalization off: no gain applied")
    }

    func testDataFolderFollowsSonarHome() {
        XCTAssertEqual(AppSettings.dataFolder(environment: ["SONAR_HOME": "/tmp/x"]).path, "/tmp/x")
        XCTAssertTrue(AppSettings.dataFolder(environment: [:]).path.hasSuffix("Library/Application Support/Sonar"))
    }
}
