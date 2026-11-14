import XCTest
@testable import Sonar

/// Scriptable `LicenseService`.
actor FakeLicenseService: LicenseService {
    var current: LicenseState
    var statusError: Error?
    var activateError: Error?
    var afterDeactivate: LicenseState

    init(current: LicenseState, afterDeactivate: LicenseState = FakeLicenseService.state(.trial, days: 14)) {
        self.current = current
        self.afterDeactivate = afterDeactivate
    }

    static func state(_ kind: LicenseStateKind, days: UInt32 = 0, devKey: Bool = false) -> LicenseState {
        LicenseState(
            kind: kind, daysLeft: days, endsAtUnix: nil, licensee: kind == .licensed ? "a@b.c" : nil,
            licenseId: nil, licenseKind: nil, expires: nil, devKey: devKey)
    }

    func setStatusError(_ e: Error?) { statusError = e }
    func setActivateError(_ e: Error?) { activateError = e }

    func status() async throws -> LicenseState {
        if let statusError { throw statusError }
        return current
    }

    func activate(key: String) async throws -> LicenseState {
        if let activateError { throw activateError }
        current = Self.state(.licensed)
        return current
    }

    func deactivate() async throws -> LicenseState {
        current = afterDeactivate
        return current
    }
}

@MainActor
final class LicenseTests: XCTestCase {
    func testTrialShowsBadgeWithDays() async {
        let manager = LicenseManager(service: FakeLicenseService(current: FakeLicenseService.state(.trial, days: 5)))
        await manager.refresh()
        XCTAssertEqual(manager.trialBadgeText, "Trial: 5 days left")
        XCTAssertFalse(manager.showsGate)
    }

    func testSingularDay() async {
        let manager = LicenseManager(service: FakeLicenseService(current: FakeLicenseService.state(.trial, days: 1)))
        await manager.refresh()
        XCTAssertEqual(manager.trialBadgeText, "Trial: 1 day left")
    }

    func testExpiredShowsGate() async {
        for kind in [LicenseStateKind.trialExpired, .licenseExpired] {
            let manager = LicenseManager(service: FakeLicenseService(current: FakeLicenseService.state(kind)))
            await manager.refresh()
            XCTAssertTrue(manager.showsGate)
            XCTAssertNil(manager.trialBadgeText)
        }
    }

    func testActivateFailureSurfacesMessage() async {
        let service = FakeLicenseService(current: FakeLicenseService.state(.trialExpired))
        await service.setActivateError(FfiError.License(message: "This key is not valid."))
        let manager = LicenseManager(service: service)
        await manager.refresh()
        let ok = await manager.activate(key: "bad")
        XCTAssertFalse(ok)
        XCTAssertEqual(manager.activationError, "This key is not valid.")
        XCTAssertTrue(manager.showsGate)
    }

    func testActivateSuccessLicensesAndClearsGate() async {
        let manager = LicenseManager(service: FakeLicenseService(current: FakeLicenseService.state(.trialExpired)))
        await manager.refresh()
        XCTAssertTrue(manager.showsGate)
        let ok = await manager.activate(key: "good")
        XCTAssertTrue(ok)
        XCTAssertEqual(manager.state?.kind, .licensed)
        XCTAssertFalse(manager.showsGate)
        XCTAssertNil(manager.trialBadgeText)
        XCTAssertNil(manager.activationError)
    }

    func testDeactivateReturnsServiceState() async {
        let service = FakeLicenseService(
            current: FakeLicenseService.state(.licensed),
            afterDeactivate: FakeLicenseService.state(.trialExpired))
        let manager = LicenseManager(service: service)
        await manager.refresh()
        await manager.deactivate()
        XCTAssertEqual(manager.state?.kind, .trialExpired)
        XCTAssertTrue(manager.showsGate)
    }

    func testIoErrorFailsOpen() async {
        let service = FakeLicenseService(current: FakeLicenseService.state(.trialExpired))
        await service.setStatusError(FfiError.Io(message: "disk"))
        let manager = LicenseManager(service: service)
        await manager.refresh()
        XCTAssertNil(manager.state)
        XCTAssertFalse(manager.showsGate)
        XCTAssertNil(manager.trialBadgeText)
    }

    func testPurchaseURLParsing() {
        XCTAssertNil(LicenseManager.purchaseURL(info: [:]))
        XCTAssertNil(LicenseManager.purchaseURL(info: ["SonarPurchaseURL": ""]))
        XCTAssertNil(LicenseManager.purchaseURL(info: ["SonarPurchaseURL": "http://x.example"]))
        XCTAssertEqual(
            LicenseManager.purchaseURL(info: ["SonarPurchaseURL": "https://x.example/buy"])?.absoluteString,
            "https://x.example/buy")
    }
}
