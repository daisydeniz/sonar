import AppKit

/// NSOpenPanel wrapper to choose a folder.
@MainActor
enum FolderPicker {
    /// Shows the panel; returns the chosen folder path or nil when cancelled.
    static func pickFolder(prompt: String = "Add Library") -> String? {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.allowsMultipleSelection = false
        panel.prompt = prompt
        panel.message = "Choose a folder of samples to add to Sonar."
        return panel.runModal() == .OK ? panel.url?.path : nil
    }
}
