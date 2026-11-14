import XCTest
@testable import Sonar

@MainActor
final class AppUpdaterTests: XCTestCase {
    func testNotConfiguredWithoutFeedOrKey() {
        XCTAssertFalse(AppUpdater.isConfigured(info: [:]))
        XCTAssertFalse(AppUpdater.isConfigured(info: ["SUFeedURL": "", "SUPublicEDKey": ""]))
        XCTAssertFalse(AppUpdater.isConfigured(info: ["SUFeedURL": "https://x/appcast.xml", "SUPublicEDKey": " "]))
        XCTAssertFalse(AppUpdater.isConfigured(info: ["SUFeedURL": "", "SUPublicEDKey": "abc"]))
    }

    func testConfiguredWithBoth() {
        XCTAssertTrue(AppUpdater.isConfigured(info: ["SUFeedURL": "https://x/appcast.xml", "SUPublicEDKey": "abc"]))
    }

    func testInertWhenUnconfigured() {
        let updater = AppUpdater(info: [:])
        XCTAssertFalse(updater.isEnabled)
        updater.checkForUpdates()  // no-op, must not crash
    }

    func testDevBundleIsInert() {
        // The hosted test app is built without feed/key.
        XCTAssertFalse(AppUpdater(start: true).isEnabled)
    }
}
