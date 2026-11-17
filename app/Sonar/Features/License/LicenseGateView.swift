import AppKit
import SwiftUI

/// Blocking view shown instead of the main window when the trial or a
/// subscription has expired. Never touches library data.
@MainActor
struct LicenseGateView: View {
    let license: LicenseManager
    var purchaseURL: URL? = LicenseManager.purchaseURL()

    private var title: String {
        license.state?.kind == .licenseExpired ? "Your license has expired" : "Your trial has ended"
    }

    private var explanation: String {
        if license.state?.kind == .licenseExpired {
            let when = license.state?.expires.map { " on \($0)" } ?? ""
            return "Your Sonar subscription ended\(when). Enter a renewed license key to keep using Sonar. Your library and index are untouched."
        }
        return "Thanks for trying Sonar. Enter a license key to continue. Your library and index are untouched."
    }

    var body: some View {
        EmptyStateView(
            symbol: "lock", title: title, message: explanation, actionTitle: "Enter License…",
            action: { license.showsEnterSheet = true },
            extra: {
                HStack(spacing: Theme.Space.s8) {
                    if let purchaseURL {
                        Button("Buy Sonar") { NSWorkspace.shared.open(purchaseURL) }
                    }
                    Button("Quit") { NSApplication.shared.terminate(nil) }
                }
            }
        )
        .defaultActionShortcut()
    }
}
