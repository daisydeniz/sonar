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

/// Where an open tag popover is anchored.
enum TagPopoverAnchor: Equatable, Sendable {
    /// The focused row of the results table (⌘T, menus).
    case row
    /// The `+` of the inspector Tags card.
    case inspector
}

/// A tag popover to show for these files.
struct TagPopoverRequest: Equatable, Sendable {
    var ids: [Int64]
    var anchor: TagPopoverAnchor
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
    /// The open tag popover (nil = closed).
    var tagPopover: TagPopoverRequest?
    /// Bumped when a tag edit (or its Undo) has reached the core and the
    /// results are fresh: an open popover reloads its state then.
    var tagsRevision = 0
    /// Live tag edits still in flight (the revision is bumped when 0).
    var tagWork = 0
    /// Undo stack of tag edits: the window's manager once `ContentView`
    /// has it (Edit > Undo), else a private one.
    var undoManager = UndoManager()
    /// The Manage Tags sheet is open (T12.8).
    var showsManageTags = false
    /// "Manage Tags…" (menus, popover footer): opens the sheet.
    var manageTags: (() -> Void)? { { [self] in showsManageTags = true } }
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
