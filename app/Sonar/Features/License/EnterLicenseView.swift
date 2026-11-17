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
        VStack(alignment: .leading, spacing: 12) {
            Text("Enter License").font(.title2.bold())
            Text("Paste the license key from your purchase e-mail.")
                .foregroundStyle(.secondary)
            TextEditor(text: $key)
                .font(.system(.body, design: .monospaced))
                .frame(height: 110)
                .overlay(RoundedRectangle(cornerRadius: 6).stroke(.quaternary))
                .accessibilityLabel("License key")
            if let error = license.activationError {
                Text(error).foregroundStyle(.red).font(.callout)
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityIdentifier("license.error")
            }
            HStack {
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
        .padding(20)
        .frame(width: 440)
        .onAppear { license.clearActivationError() }
    }
}
