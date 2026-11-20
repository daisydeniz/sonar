import SwiftUI

/// Toolbar badge "Trial: N days left" (DESIGN §3): `signal` text on
/// `signalSoft`, `caption` semibold; opens the Enter License sheet.
@MainActor
struct TrialBadgeView: View {
    let license: LicenseManager

    var body: some View {
        if let text = license.trialBadgeText {
            Button { license.showsEnterSheet = true } label: {
                Text(text)
                    .font(Theme.Font.caption.weight(.semibold))
                    .foregroundStyle(Theme.Color.signal)
                    .padding(.horizontal, Theme.Space.s8)
                    .padding(.vertical, Theme.Space.s2)
                    .background(Theme.Color.signalSoft, in: Capsule())
            }
            .buttonStyle(.plain)
            .help("Enter a license key")
            .accessibilityLabel("\(text). Enter license")
        }
    }
}
