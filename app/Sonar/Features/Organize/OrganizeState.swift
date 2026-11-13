import Foundation
import Observation

/// A text prompt the window shows as an alert with one text field.
enum OrganizePrompt: Equatable, Sendable {
    /// Add the typed (comma-separated) tags to these files.
    case addTag([Int64])
    /// Create a collection, then add these files to it (may be empty).
    case newCollection([Int64])
    /// Rename this collection.
    case renameCollection(Int64)

    var title: String {
        switch self {
        case .addTag: return "Add Tag"
        case .newCollection: return "New Collection"
        case .renameCollection: return "Rename Collection"
        }
    }

    var message: String {
        switch self {
        case .addTag: return "Separate several tags with commas."
        case .newCollection: return "Name of the new collection."
        case .renameCollection: return "New name of the collection."
        }
    }

    var confirmTitle: String {
        switch self {
        case .addTag: return "Add"
        case .newCollection: return "Create"
        case .renameCollection: return "Rename"
        }
    }
}

/// View state of the organize features (T6.8): the "show hidden" switch, the
/// pending text prompt, the inline override error, tag suggestions. The work
/// itself is in `AppModel+Organize`.
@MainActor
@Observable
final class OrganizeState {
    /// Include hidden samples in results (shown dimmed, offering "Unhide").
    var showHidden = false
    /// Pending alert (nil = none) and its text field.
    var prompt: OrganizePrompt?
    var promptText = ""
    /// Last override rejection from the core, shown inline in the inspector.
    var overrideError: String?
    /// Every tag in the library with its file count (suggestions).
    internal(set) var tags: [TagInfo] = []
    /// Valid category ids in taxonomy order (the category popup).
    let categories: [String] = categoryIds()

    /// Opens a prompt with an initial text.
    func ask(_ prompt: OrganizePrompt, text: String = "") {
        promptText = text
        self.prompt = prompt
    }
}
