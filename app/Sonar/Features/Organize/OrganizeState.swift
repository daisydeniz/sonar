import Foundation
import Observation

/// A text prompt the window shows as an alert with one text field.
enum OrganizePrompt: Equatable, Sendable {
    /// Create a collection, then add these files to it (may be empty).
    case newCollection([Int64])
    /// Rename this collection.
    case renameCollection(Int64)

    var title: String {
        switch self {
        case .newCollection: return "New Collection"
        case .renameCollection: return "Rename Collection"
        }
    }

    var message: String {
        switch self {
        case .newCollection: return "Name of the new collection."
        case .renameCollection: return "New name of the collection."
        }
    }

    var confirmTitle: String {
        switch self {
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
    /// Files the tag editor sheet is open for (nil = closed).
    var tagEditorTargets: [Int64]?
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
