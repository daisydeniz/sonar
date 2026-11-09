import SwiftUI

/// Application entry point. Owns the main window scene.
@main
struct SonarApp: App {
    var body: some Scene {
        WindowGroup(AppInfo.name) {
            ContentView()
                .frame(minWidth: 720, minHeight: 440)
        }
    }
}
