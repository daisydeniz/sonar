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
        VStack(spacing: 14) {
            Image(systemName: "lock.circle").font(.system(size: 48)).foregroundStyle(.secondary)
                .accessibilityHidden(true)
            Text(title).font(.title.bold())
            Text(explanation).multilineTextAlignment(.center).foregroundStyle(.secondary)
                .frame(maxWidth: 420)
            HStack {
                Button("Enter License…") { license.showsEnterSheet = true }
                    .keyboardShortcut(.defaultAction)
                if let purchaseURL {
                    Button("Buy Sonar") { NSWorkspace.shared.open(purchaseURL) }
                }
                Button("Quit") { NSApplication.shared.terminate(nil) }
            }
            .padding(.top, 6)
        }
        .padding(40)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .accessibilityElement(children: .contain)
    }
}
