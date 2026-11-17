import SwiftUI

/// Shared empty / unavailable state (DESIGN §4.6): 28 pt tertiary symbol,
/// title, one secondary line and at most one primary action. Centred.
/// `extra` holds quieter content below the action (secondary buttons, a hint).
struct EmptyStateView<Extra: View>: View {
    let symbol: String
    let title: String
    let message: String
    var actionTitle: String?
    var action: (() -> Void)?
    @ViewBuilder var extra: () -> Extra
    /// Return triggers the primary action (full-window states only).
    private var actionIsDefault = false

    init(
        symbol: String, title: String, message: String, actionTitle: String? = nil,
        action: (() -> Void)? = nil, @ViewBuilder extra: @escaping () -> Extra
    ) {
        self.symbol = symbol
        self.title = title
        self.message = message
        self.actionTitle = actionTitle
        self.action = action
        self.extra = extra
    }

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
                primaryButton(actionTitle, action: action)
                    .buttonStyle(.borderedProminent)
                    .controlSize(.regular)
                    .padding(.top, Theme.Space.s8)
            }
            extra()
        }
        .padding(Theme.Space.s32)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .accessibilityElement(children: .contain)
    }

    @ViewBuilder
    private func primaryButton(_ title: String, action: @escaping () -> Void) -> some View {
        if actionIsDefault {
            Button(title, action: action).keyboardShortcut(.defaultAction)
        } else {
            Button(title, action: action)
        }
    }

    /// Makes Return trigger the primary action.
    func defaultActionShortcut() -> Self {
        var copy = self
        copy.actionIsDefault = true
        return copy
    }
}

extension EmptyStateView where Extra == EmptyView {
    init(
        symbol: String, title: String, message: String, actionTitle: String? = nil,
        action: (() -> Void)? = nil
    ) {
        self.init(
            symbol: symbol, title: title, message: message, actionTitle: actionTitle, action: action,
            extra: { EmptyView() })
    }
}
