import SwiftUI

/// Thin banner above the results: an engine event error with a dismiss button.
@MainActor
struct StatusBannerView: View {
    let banner: StatusBanner

    var body: some View {
        if let text = banner.displayText {
            HStack(spacing: 8) {
                Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(.orange)
                Text(text).lineLimit(2).textSelection(.enabled)
                Spacer()
                Button { banner.dismiss() } label: { Image(systemName: "xmark") }
                    .buttonStyle(.borderless)
                    .accessibilityLabel("Dismiss message")
            }
            .font(.callout)
            .padding(.horizontal, 12).padding(.vertical, 6)
            .background(.orange.opacity(0.12))
            .accessibilityElement(children: .contain)
            Divider()
        }
    }
}
