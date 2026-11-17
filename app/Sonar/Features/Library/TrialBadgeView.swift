import SwiftUI

/// Toolbar badge "Trial: N days left"; opens the Enter License sheet.
@MainActor
struct TrialBadgeView: View {
    let license: LicenseManager

    var body: some View {
        if let text = license.trialBadgeText {
            Button { license.showsEnterSheet = true } label: {
                Text(text).font(Theme.Font.caption)
            }
            .buttonStyle(.bordered)
            .controlSize(.small)
            .help("Enter a license key")
            .accessibilityLabel("\(text). Enter license")
        }
    }
}
