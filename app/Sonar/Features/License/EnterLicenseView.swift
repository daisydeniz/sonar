import SwiftUI

/// Sheet to paste and activate a license key. Multi-line paste is fine: the
/// core ignores whitespace.
@MainActor
struct EnterLicenseView: View {
    let license: LicenseManager
    let dismiss: () -> Void
    @State private var key = ""

    private var canActivate: Bool {
        !key.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && !license.isActivating
    }

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.s12) {
            Text("Enter License").font(Theme.Font.title)
            Text("Paste the license key from your purchase e-mail.")
                .font(Theme.Font.body).foregroundStyle(.secondary)
            TextEditor(text: $key)
                .font(Theme.Font.row.monospaced())
                .scrollContentBackground(.hidden)
                .padding(Theme.Space.s4)
                .frame(height: 110)
                .background(Theme.Color.surfaceGroup, in: RoundedRectangle(cornerRadius: Theme.Radius.medium))
                .overlay {
                    RoundedRectangle(cornerRadius: Theme.Radius.medium)
                        .strokeBorder(Theme.Color.hairline, lineWidth: Theme.hairlineWidth)
                }
                .accessibilityLabel("License key")
            if let error = license.activationError {
                Text(error).foregroundStyle(Theme.Color.error).font(Theme.Font.row)
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityIdentifier("license.error")
            }
            HStack(spacing: Theme.Space.s8) {
                if license.isActivating { ProgressView().controlSize(.small) }
                Spacer()
                Button("Cancel", role: .cancel, action: dismiss).keyboardShortcut(.cancelAction)
                Button("Activate") {
                    Task { if await license.activate(key: key) { dismiss() } }
                }
                .keyboardShortcut(.defaultAction)
                .disabled(!canActivate)
            }
        }
        .padding(Theme.Space.s24)
        .frame(width: 440)
        .onAppear { license.clearActivationError() }
    }
}
