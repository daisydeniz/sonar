import AppKit

/// NSOpenPanel wrapper to choose folders.
@MainActor
enum FolderPicker {
    /// Shows the panel; returns the chosen folder path or nil when cancelled.
    static func pickFolder(prompt: String = "Add Library") -> String? {
        pickFolders(prompt: prompt, multiple: false).first
    }

    /// Shows the panel (folders only, optionally several); empty when cancelled.
    static func pickFolders(prompt: String = "Add Library", multiple: Bool = true) -> [String] {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.allowsMultipleSelection = multiple
        panel.prompt = prompt
        panel.message = multiple
            ? "Choose folders of samples to add to Sonar."
            : "Choose a folder of samples to add to Sonar."
        return panel.runModal() == .OK ? panel.urls.map(\.path) : []
    }
}
