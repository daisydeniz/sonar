import XCTest
@testable import Sonar

final class AppInfoTests: XCTestCase {
    func testNameIsSonar() {
        XCTAssertEqual(AppInfo.name, "Sonar")
    }

    func testVersionStringFromHostBundle() {
        // Tests run hosted in Sonar.app, so Bundle.main is the app bundle.
        let version = AppInfo.versionString()
        XCTAssertFalse(version.isEmpty)
        XCTAssertTrue(version.hasPrefix("0.1.0"), "unexpected version: \(version)")
    }

    func testVersionStringFallsBackWithoutInfo() {
        XCTAssertEqual(AppInfo.versionString(info: [:]), "0.0.0 (0)")
    }

    func testVersionStringFormatsKeys() {
        let info: [String: Any] = ["CFBundleShortVersionString": "1.2.3", "CFBundleVersion": "42"]
        XCTAssertEqual(AppInfo.versionString(info: info), "1.2.3 (42)")
    }
}
