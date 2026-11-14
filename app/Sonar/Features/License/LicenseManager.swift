import Foundation
import Observation
import os

/// Holds the current licensing state for the UI. Presentation only: the core
/// decides trial/expiry. Refreshes on launch and when the app becomes active.
@MainActor
@Observable
final class LicenseManager {
    private static let log = Logger(subsystem: "app.sonar.Sonar", category: "license")

    /// Last state reported by the service; nil until the first successful call
    /// (or after a failed one: the app then fails open).
    private(set) var state: LicenseState?
    /// Message of the last failed activation, shown inline in the sheet.
    private(set) var activationError: String?
    /// Message of the last failed deactivation.
    private(set) var deactivationError: String?
    /// True while an activation call is running.
    private(set) var isActivating = false
    /// Drives the Enter License sheet in the main window.
    var showsEnterSheet = false

    @ObservationIgnored private let service: any LicenseService
    @ObservationIgnored private var generation = 0

    init(service: any LicenseService = CoreLicenseService()) {
        self.service = service
    }

    /// True when the blocking gate must replace the main window. A failed
    /// license call leaves `state` nil, so an I/O error never locks the user out.
    var showsGate: Bool {
        guard let kind = state?.kind else { return false }
        return kind == .trialExpired || kind == .licenseExpired
    }

    /// "Trial: N days left" while the trial runs; nil otherwise (hidden when licensed).
    var trialBadgeText: String? {
        guard let state, state.kind == .trial else { return nil }
        return state.daysLeft == 1 ? "Trial: 1 day left" : "Trial: \(state.daysLeft) days left"
    }

    /// Re-reads the state from the service. Errors fail open (logged).
    func refresh() async {
        generation += 1
        let mine = generation
        do {
            let new = try await service.status()
            if mine == generation { state = new }
        } catch {
            Self.log.error("license status failed, failing open: \(String(describing: error), privacy: .public)")
            if mine == generation { state = nil }
        }
    }

    /// Activates `key`. Returns true on success (state updated, sheet may close);
    /// on failure `activationError` carries the core's user-facing message.
    @discardableResult
    func activate(key: String) async -> Bool {
        guard !isActivating else { return false }
        isActivating = true
        activationError = nil
        defer { isActivating = false }
        do {
            let new = try await service.activate(key: key)
            generation += 1
            state = new
            return true
        } catch {
            activationError = Self.message(for: error)
            Self.log.error("license activation failed: \(String(describing: error), privacy: .public)")
            return false
        }
    }

    /// Removes the stored key; the state becomes whatever the service reports.
    func deactivate() async {
        deactivationError = nil
        do {
            let new = try await service.deactivate()
            generation += 1
            state = new
        } catch {
            deactivationError = Self.message(for: error)
            Self.log.error("license deactivation failed: \(String(describing: error), privacy: .public)")
        }
    }

    /// Clears a stale inline error (call when the sheet opens).
    func clearActivationError() { activationError = nil }

    /// User-facing text of a service error.
    nonisolated static func message(for error: Error) -> String {
        if let ffi = error as? FfiError {
            switch ffi {
            case .License(let message), .Io(let message), .InvalidInput(let message): return message
            default: break
            }
        }
        return error.localizedDescription
    }

    /// Purchase page from Info.plist `SonarPurchaseURL`; nil when empty or invalid.
    nonisolated static func purchaseURL(info: [String: Any] = Bundle.main.infoDictionary ?? [:]) -> URL? {
        let raw = (info["SonarPurchaseURL"] as? String)?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        guard !raw.isEmpty, let url = URL(string: raw), url.scheme == "https", url.host != nil else { return nil }
        return url
    }
}
