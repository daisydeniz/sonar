import XCTest
@testable import Sonar

/// Proves the Rust static lib links into the app and the UniFFI call works at
/// runtime (tests run hosted inside Sonar.app).
final class CoreBridgeTests: XCTestCase {
    func testCoreVersionIsSemver() {
        let version = CoreInfo.version
        XCTAssertFalse(version.isEmpty)
        let parts = version.split(separator: ".", omittingEmptySubsequences: false)
        XCTAssertEqual(parts.count, 3, "not semver-like: \(version)")
        for part in parts {
            XCTAssertNotNil(UInt(part), "non-numeric component in \(version)")
        }
    }

    func testCoreVersionMatchesDirectFFICall() {
        XCTAssertEqual(CoreInfo.version, coreVersion())
    }
}
