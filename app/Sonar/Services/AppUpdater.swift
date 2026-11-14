import Foundation
import Sparkle

/// Sparkle 2 wrapper (ADR-037). The updater only starts when the bundle
/// carries both `SUFeedURL` and `SUPublicEDKey`, which only release builds
/// get (scripts/release.sh). Dev builds and unit tests stay inert: no
/// network, no prompts, and "Check for Updates…" is disabled.
@MainActor
final class AppUpdater {
    /// True when the Info.plist has a non-empty feed URL and public key.
    nonisolated static func isConfigured(info: [String: Any]) -> Bool {
        func value(_ key: String) -> String {
            (info[key] as? String)?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        }
        return !value("SUFeedURL").isEmpty && !value("SUPublicEDKey").isEmpty
    }

    let isEnabled: Bool
    private let controller: SPUStandardUpdaterController?

    /// - Parameter start: pass false under test to never touch Sparkle.
    init(info: [String: Any] = Bundle.main.infoDictionary ?? [:], start: Bool = true) {
        isEnabled = start && Self.isConfigured(info: info)
        controller = isEnabled
            ? SPUStandardUpdaterController(startingUpdater: true, updaterDelegate: nil, userDriverDelegate: nil)
            : nil
    }

    /// User-initiated check (app menu).
    func checkForUpdates() {
        controller?.checkForUpdates(nil)
    }
}
