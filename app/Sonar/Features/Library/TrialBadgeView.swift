import SwiftUI

/// Toolbar badge "Trial: N days left"; opens the Enter License sheet.
@MainActor
struct TrialBadgeView: View {
    let license: LicenseManager

    var body: some View {
        if let text = license.trialBadgeText {
            // Default toolbar button style: the system draws the capsule and its
            // insets (a bordered style inside a toolbar item double-wraps it).
            Button { license.showsEnterSheet = true } label: {
                Text(text).font(Theme.Font.caption).padding(.horizontal, Theme.Space.s4)
            }
            .help("Enter a license key")
            .accessibilityLabel("\(text). Enter license")
        }
    }
}
