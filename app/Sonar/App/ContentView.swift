import SwiftUI

/// Placeholder main window content. Replaced by the library/search/results
/// layout in later milestones; T0.5 adds the core version via the UniFFI bridge.
@MainActor
struct ContentView: View {
    var body: some View {
        VStack(spacing: 8) {
            Text(AppInfo.name)
                .font(.largeTitle.weight(.semibold))
                .accessibilityAddTraits(.isHeader)
            Text("Sample library search is on its way.")
                .foregroundStyle(.secondary)
            Text("Version \(AppInfo.versionString())")
                .font(.caption)
                .foregroundStyle(.tertiary)
                .accessibilityLabel("App version \(AppInfo.versionString())")
        }
        .padding(40)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

#Preview {
    ContentView()
}
