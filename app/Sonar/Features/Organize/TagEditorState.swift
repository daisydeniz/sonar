import Foundation

/// Editing state of the tag editor sheet (T10.9): pure Swift, no core.
///
/// `initial` are the tags every target file already has (the intersection),
/// `chips` the tags that will be on every target after Save. Tags only some
/// files have are neither chips nor ever removed. Names are matched
/// case-insensitively and an existing tag's spelling always wins.
struct TagEditorState: Equatable {
    /// One line of the suggestion list.
    enum Row: Equatable {
        /// "Create “x”": a tag that exists nowhere yet.
        case create(String)
        /// A library tag (or a tag just created in this sheet, `files` 0).
        case existing(name: String, files: Int, chosen: Bool)
    }

    let library: [TagInfo]
    let initial: [String]
    private(set) var chips: [String]
    /// The inline field's text.
    var text = "" { didSet { if text != oldValue { highlight = text.isEmpty ? nil : 0 } } }
    /// Highlighted index into `rows` (nil = none).
    private(set) var highlight: Int?

    init(library: [TagInfo], common: [String]) {
        self.library = library
        initial = common
        chips = common
    }

    // MARK: Matching

    private static func same(_ a: String, _ b: String) -> Bool {
        a.compare(b, options: [.caseInsensitive, .diacriticInsensitive]) == .orderedSame
    }

    /// Library tags plus chips the library does not know yet.
    private var candidates: [(name: String, files: Int)] {
        var list = library.map { (name: $0.name, files: Int($0.files)) }
        for chip in chips where !list.contains(where: { Self.same($0.name, chip) }) {
            list.append((chip, 0))
        }
        return list
    }

    /// The spelling to use for `name`: the existing tag's, else the trimmed input.
    func canonical(_ name: String) -> String {
        let name = name.trimmingCharacters(in: .whitespaces)
        return candidates.first { Self.same($0.name, name) }?.name ?? name
    }

    func isChosen(_ name: String) -> Bool { chips.contains { Self.same($0, name) } }

    // MARK: List

    /// Suggestion list for the current text: the create row when no tag
    /// matches exactly, else the exact match first; then prefix matches, then
    /// other substring matches (so Return on a typed name picks that tag).
    var rows: [Row] {
        let query = text.trimmingCharacters(in: .whitespaces)
        let all = candidates
        guard !query.isEmpty else {
            return all.map { .existing(name: $0.name, files: $0.files, chosen: isChosen($0.name)) }
        }
        let opts: String.CompareOptions = [.caseInsensitive, .diacriticInsensitive]
        func rank(_ name: String) -> Int? {
            if Self.same(name, query) { return 0 }
            if name.range(of: query, options: opts.union(.anchored)) != nil { return 1 }
            return name.range(of: query, options: opts) != nil ? 2 : nil
        }
        let ranked = all.compactMap { item in rank(item.name).map { (item, $0) } }
        var result: [Row] = ranked.contains { $0.1 == 0 } ? [] : [.create(query)]
        for tier in 0...2 {
            for (item, r) in ranked where r == tier {
                result.append(.existing(name: item.name, files: item.files, chosen: isChosen(item.name)))
            }
        }
        return result
    }

    mutating func moveHighlight(by delta: Int) {
        let count = rows.count
        guard count > 0 else { highlight = nil; return }
        guard let current = highlight else { highlight = delta > 0 ? 0 : count - 1; return }
        highlight = min(max(current + delta, 0), count - 1)
    }

    // MARK: Edits

    /// Adds the tag when absent, removes it when present.
    mutating func toggle(_ name: String) {
        let name = canonical(name)
        guard !name.isEmpty else { return }
        if let i = chips.firstIndex(where: { Self.same($0, name) }) { chips.remove(at: i) } else { chips.append(name) }
    }

    /// Adds the tag (no-op when already chosen).
    mutating func add(_ name: String) {
        let name = canonical(name)
        guard !name.isEmpty, !isChosen(name) else { return }
        chips.append(name)
    }

    mutating func remove(_ name: String) { chips.removeAll { Self.same($0, name) } }

    /// ⌫ on an empty field.
    mutating func removeLast() { if !chips.isEmpty { chips.removeLast() } }

    /// Return: toggles / creates the highlighted row and clears the text.
    mutating func activateHighlighted() {
        let list = rows
        guard let h = highlight, h < list.count else { return }
        switch list[h] {
        case .create(let name): add(name)
        case .existing(let name, _, _): toggle(name)
        }
        text = ""
    }

    /// Comma or Tab: the typed text becomes chips (comma separated, so a
    /// paste of "a, b, c" gives three).
    mutating func commitText() {
        for part in text.split(separator: ",") { add(String(part)) }
        text = ""
    }

    /// The field changed: complete comma-terminated pieces become chips and
    /// only the text after the last comma stays in the field.
    mutating func textChanged() {
        guard text.contains(",") else { return }
        var parts = text.split(separator: ",", omittingEmptySubsequences: false).map(String.init)
        let rest = parts.removeLast()
        for part in parts { add(part) }
        text = rest.trimmingCharacters(in: .whitespaces)
    }

    // MARK: Result

    /// Tags to add to every target (not in `initial`).
    var added: [String] { chips.filter { chip in !initial.contains { Self.same($0, chip) } } }
    /// Common tags the user took off.
    var removed: [String] { initial.filter { !isChosen($0) } }
    var hasChanges: Bool { !added.isEmpty || !removed.isEmpty }
}
