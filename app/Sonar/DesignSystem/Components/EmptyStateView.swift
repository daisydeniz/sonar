import SwiftUI

/// Shared empty / unavailable state (DESIGN §4.6): 28 pt tertiary symbol,
/// title, one secondary line and at most one primary action. Centred.
struct EmptyStateView: View {
    let symbol: String
    let title: String
    let message: String
    var actionTitle: String?
    var action: (() -> Void)?

    var body: some View {
        VStack(spacing: Theme.Space.s8) {
            Image(systemName: symbol)
                .font(.system(size: Theme.Size.emptyStateSymbol))
                .foregroundStyle(.tertiary)
                .accessibilityHidden(true)
            Text(title).font(Theme.Font.title).foregroundStyle(.primary)
            Text(message)
                .font(Theme.Font.body).foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
                .frame(maxWidth: Theme.Size.emptyStateTextWidth)
            if let actionTitle, let action {
                Button(actionTitle, action: action)
                    .buttonStyle(.borderedProminent)
                    .controlSize(.regular)
                    .padding(.top, Theme.Space.s8)
            }
        }
        .padding(Theme.Space.s32)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .accessibilityElement(children: .combine)
    }
}
