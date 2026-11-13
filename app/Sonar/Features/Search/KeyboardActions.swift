import AppKit

/// Keys the results table handles itself (arrows are native NSTableView).
enum TableKey: Equatable, Sendable {
    case space, returnKey, escape

    /// Maps a key-down event (virtual key codes: space 49, return 36,
    /// keypad enter 76, escape 53); modified keys (⌘/⌃/⌥) are not ours.
    static func from(_ event: NSEvent) -> TableKey? {
        guard event.type == .keyDown,
            event.modifierFlags.intersection([.command, .control, .option]).isEmpty
        else { return nil }
        switch event.keyCode {
        case 49: return .space
        case 36, 76: return .returnKey
        case 53: return .escape
        default: return nil
        }
    }
}

/// Keyboard-first hooks between the table, the search field and the model.
/// All closures are injectable so tests (and T6.4) can observe or replace them.
@MainActor
final class KeyboardActions {
    /// Space in the table. T6.4 (preview) wires this to play/stop; until then
    /// the request is only counted.
    var onTogglePreview: (() -> Void)?
    private(set) var previewToggleRequests = 0
    /// Return in the table: reveal files in Finder.
    var reveal: ([URL]) -> Void = { NSWorkspace.shared.activateFileViewerSelecting($0) }
    /// Registered by the table: focus it (selecting row 0 if nothing is
    /// selected). Returns false when there is nothing to focus (no rows).
    var focusResults: (() -> Bool)?
    /// Focus the toolbar search field (⌘F, Esc in the table).
    var focusSearch: () -> Void = { SearchFieldFocus.focus() }

    func togglePreview() {
        previewToggleRequests += 1
        onTogglePreview?()
    }

    /// Reveals the whole multi-selection, or just the focused row.
    func revealSelected(_ row: FileRow?, selection: [FileRow] = []) {
        let rows = selection.count > 1 ? selection : (row.map { [$0] } ?? [])
        guard !rows.isEmpty else { return }
        reveal(rows.map { URL(fileURLWithPath: $0.path) })
    }

    /// Routes a table key; returns whether it was handled.
    @discardableResult
    func handle(_ key: TableKey, selected: FileRow?, selection: [FileRow] = []) -> Bool {
        switch key {
        case .space: togglePreview()
        case .returnKey: revealSelected(selected, selection: selection)
        case .escape: focusSearch()
        }
        return true
    }
}

/// Finds and focuses the window's search field. `.searchable` offers no
/// focus API on macOS 14, so this walks the titlebar/toolbar views.
@MainActor
enum SearchFieldFocus {
    static func focus() {
        guard let window = NSApp.keyWindow ?? NSApp.mainWindow,
            let root = window.contentView?.superview,
            let field = findSearchField(in: root)
        else { return }
        window.makeFirstResponder(field)
    }

    static func findSearchField(in view: NSView) -> NSSearchField? {
        if let field = view as? NSSearchField { return field }
        for sub in view.subviews { if let f = findSearchField(in: sub) { return f } }
        return nil
    }

    /// True when `responder` is (the field editor of) a search field.
    static func isSearchField(_ responder: NSResponder?) -> Bool {
        if responder is NSSearchField { return true }
        if let text = responder as? NSText, text.delegate is NSSearchField { return true }
        return false
    }
}

/// Local key monitor: ↓ in the search field moves focus to the results.
/// (Return is handled by `.onSubmit(of: .search)` in `ContentView`.)
@MainActor
final class SearchKeyMonitor {
    private var token: Any?

    func install(actions: KeyboardActions) {
        guard token == nil else { return }
        token = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { event in
            // keyCode 125 = down arrow, no modifiers.
            if event.keyCode == 125,
                event.modifierFlags.intersection([.command, .control, .option, .shift]).isEmpty,
                SearchFieldFocus.isSearchField(event.window?.firstResponder)
            {
                let took = MainActor.assumeIsolated { actions.focusResults?() ?? false }
                return took ? nil : event
            }
            return event
        }
    }

    func remove() {
        if let token { NSEvent.removeMonitor(token) }
        token = nil
    }
}
