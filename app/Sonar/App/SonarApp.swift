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
            CommandMenu("Samples") {
                Button("Favorite / Unfavorite") { Task { await model.toggleFavoriteOfSelection() } }
                    .keyboardShortcut("l", modifiers: .command)
                    .disabled(model.selectedRow == nil)
                Button("Hide / Unhide") { Task { await model.toggleHiddenOfSelection() } }
                    .keyboardShortcut("h", modifiers: [.command, .shift])
                    .disabled(model.selectedRow == nil)
                Button("Add Tag…") { model.organizeState.ask(.addTag(model.organizeTargetIDs)) }
                    .disabled(model.selectedRow == nil)
            }
            CommandGroup(after: .toolbar) {
                Toggle("Show Hidden Samples", isOn: Bindable(model.organizeState).showHidden)
                    .keyboardShortcut(".", modifiers: [.command, .shift])
            }
            CommandGroup(after: .newItem) {
                Button("Add Library…") {
                    let paths = FolderPicker.pickFolders()
                    if !paths.isEmpty { Task { await model.addLibraries(paths: paths) } }
                }
                .keyboardShortcut("o", modifiers: [.command, .shift])
            }
        }
        Settings { SettingsView(model: model) }
    }
}
