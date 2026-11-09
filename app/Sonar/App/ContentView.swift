import SwiftUI

/// Placeholder main window content. Replaced by the library/search/results
/// layout in later milestones. Shows the app version and the Rust core
/// version (UniFFI bridge, T0.5) as proof that the core is linked.
@MainActor
struct ContentView: View {
    var body: some View {
        VStack(spacing: 8) {
            Text(AppInfo.name)
                .font(.largeTitle.weight(.semibold))
                .accessibilityAddTraits(.isHeader)
            Text("Sample library search is on its way.")
                .foregroundStyle(.secondary)
            VStack(spacing: 2) {
                Text("Version \(AppInfo.versionString())")
                    .accessibilityLabel("App version \(AppInfo.versionString())")
                Text("Core \(CoreInfo.version)")
                    .accessibilityLabel("Core version \(CoreInfo.version)")
            }
            .font(.caption)
            .foregroundStyle(.tertiary)
        }
        .padding(40)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

#Preview {
    ContentView()
}
