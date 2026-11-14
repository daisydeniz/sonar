import SwiftUI

/// Settings "License" tab: state, licensee, id, kind/expiry, change/deactivate.
@MainActor
struct LicenseSettingsView: View {
    let license: LicenseManager
    @State private var showsSheet = false
    @State private var confirmsDeactivate = false

    var body: some View {
        Form {
            if let state = license.state {
                LabeledContent("Status", value: Self.statusText(state))
                if let licensee = state.licensee { LabeledContent("Licensee", value: licensee) }
                if let id = state.licenseId { LabeledContent("License ID", value: id) }
                if let kind = state.licenseKind { LabeledContent("Type", value: kind.capitalized) }
                if let expires = state.expires { LabeledContent("Valid until", value: expires) }
                if state.devKey {
                    Text("Development key: this build accepts the public development license key. Release builds do not.")
                        .font(.caption).foregroundStyle(.secondary)
                }
            } else {
                LabeledContent("Status", value: "Unavailable")
            }
            if let error = license.deactivationError {
                Text(error).foregroundStyle(.red).font(.callout)
            }
            HStack {
                Button(isLicensed ? "Change License…" : "Enter License…") { showsSheet = true }
                if isLicensed {
                    Button("Deactivate…", role: .destructive) { confirmsDeactivate = true }
                }
            }
        }
        .formStyle(.grouped)
        .sheet(isPresented: $showsSheet) {
            EnterLicenseView(license: license) { showsSheet = false }
        }
        .confirmationDialog("Deactivate this license?", isPresented: $confirmsDeactivate) {
            Button("Deactivate", role: .destructive) { Task { await license.deactivate() } }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text("Sonar returns to its trial status on this Mac. You can enter the key again later.")
        }
    }

    private var isLicensed: Bool {
        guard let kind = license.state?.kind else { return false }
        return kind == .licensed || kind == .licenseExpired
    }

    /// One-line description of a state.
    static func statusText(_ state: LicenseState) -> String {
        switch state.kind {
        case .trial: return state.daysLeft == 1 ? "Trial, 1 day left" : "Trial, \(state.daysLeft) days left"
        case .trialExpired: return "Trial ended"
        case .licensed: return "Licensed"
        case .licenseExpired: return "License expired"
        }
    }
}
