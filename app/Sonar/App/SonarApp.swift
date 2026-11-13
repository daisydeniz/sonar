import SwiftUI

/// Application entry point. Owns the main window scene and the app model.
@main
struct SonarApp: App {
    @State private var model = AppModel()

    /// Unit tests host the app; they build their own models, so the app
    /// must not open the user's real library then.
    private static let isTesting = ProcessInfo.processInfo.environment["XCTestConfigurationFilePath"] != nil

    var body: some Scene {
        // Single main window: one engine and one poll loop per app.
        Window(AppInfo.name, id: "main") {
            ContentView(model: model)
                .frame(minWidth: 900, minHeight: 480)
                .task { if !Self.isTesting { await model.start() } }
        }
        .commands {
            CommandGroup(replacing: .textEditing) {
                Button("Find Samples") { model.keyboard.focusSearch() }
                    .keyboardShortcut("f", modifiers: .command)
                Button("Find Similar") { model.findSimilarToSelection() }
                    .keyboardShortcut("f", modifiers: [.command, .shift])
                    .disabled(model.selectedRow == nil)
            }
            CommandGroup(after: .newItem) {
                Button("Add Library…") {
                    if let path = FolderPicker.pickFolder() {
                        Task { await model.addLibrary(path: path) }
                    }
                }
                .keyboardShortcut("o", modifiers: [.command, .shift])
            }
        }
    }
}
