import SwiftUI

/// Banner above the results (DESIGN §4.7): one line on `surfaceRaised`, a
/// leading status symbol in the system status colour, `row` text and a
/// trailing dismiss button.
@MainActor
struct StatusBannerView: View {
    let banner: StatusBanner

    var body: some View {
        if let text = banner.displayText {
            HStack(spacing: Theme.Space.s8) {
                Image(systemName: "exclamationmark.triangle.fill")
                    .imageScale(.small)
                    .foregroundStyle(Theme.Color.warning)
                    .accessibilityHidden(true)
                Text(text)
                    .font(Theme.Font.row)
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .textSelection(.enabled)
                    .help(text)
                Spacer(minLength: Theme.Space.s8)
                Button { banner.dismiss() } label: { Image(systemName: "xmark") }
                    .buttonStyle(.borderless)
                    .controlSize(.small)
                    .help("Dismiss")
                    .accessibilityLabel("Dismiss message")
            }
            .padding(.horizontal, Theme.Space.s12)
            .frame(height: Theme.Size.barHeight)
            .background(Theme.Color.surfaceRaised)
            .accessibilityElement(children: .contain)
            Hairline()
        }
    }
}
