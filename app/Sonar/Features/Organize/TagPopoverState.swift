import Foundation

/// One live tag edit: add or remove `tag` on exactly `fileIds` (the files
/// that lacked it, or had it). Its inverse is the Undo step (T12.7).
struct TagChange: Equatable, Sendable {
    enum Kind: Equatable, Sendable { case add, remove }
    var kind: Kind
    var tag: String
    var fileIds: [Int64]

    var inverse: TagChange { TagChange(kind: kind == .add ? .remove : .add, tag: tag, fileIds: fileIds) }

    /// Undo of a whole step: the inverse changes in reverse order.
    static func inverse(of changes: [TagChange]) -> [TagChange] { changes.reversed().map(\.inverse) }
}

/// Which of the target files carry a tag (menu mark, chip, list check).
enum TagMark: Equatable, Sendable {
    case none
    /// On `count` of the targets, not all.
    case some(Int)
    case all
}

/// State of the tag popover and the Tags menu (DESIGN §4.12): pure Swift,
/// no core. It knows the library's tags with counts and each target file's
/// tags; every edit changes the state at once and returns the `TagChange`s
/// the caller applies (live, one Undo step each). Names are matched
/// case-insensitively and an existing tag's spelling always wins.
struct TagPopoverState {
    /// One line of the list under the field.
    enum Row: Equatable {
        /// A library tag with its file count and mark for the targets.
        case tag(name: String, files: Int, mark: TagMark)
        /// "Create tag “x”": last row, only when no tag matches exactly.
        case create(String)
    }

    /// A chip in the field: a tag some or all targets have.
    struct Chip: Equatable {
        var name: String
        var mark: TagMark
    }

    private(set) var library: [(name: String, files: Int)]
    private(set) var files: [(id: Int64, tags: [String])]
    /// The inline field's text.
    var text = "" {
        didSet {
            guard text != oldValue else { return }
            highlight = text.isEmpty ? nil : typedRowIndex
            highlightMoved = false
        }
    }
    /// Highlighted index into `rows` (nil = none).
    private(set) var highlight: Int?
    /// True once ↑/↓ picked a row; until then Return acts on the typed text.
    private var highlightMoved = false
    /// How many tags "Most used" shows (also the menu).
    static let mostUsedCount = 8

    init(library: [TagInfo], files: [(id: Int64, tags: [String])]) {
        self.library = Self.sorted(library.map { (name: $0.name, files: Int($0.files)) })
        self.files = files
    }

    /// Fresh library and file tags (after a core refresh or an Undo); keeps
    /// the typed text.
    mutating func reload(library: [TagInfo], files: [(id: Int64, tags: [String])]) {
        self.library = Self.sorted(library.map { (name: $0.name, files: Int($0.files)) })
        self.files = files
    }

    private static func sorted(_ list: [(name: String, files: Int)]) -> [(name: String, files: Int)] {
        list.sorted { a, b in
            a.files != b.files ? a.files > b.files : a.name.localizedCaseInsensitiveCompare(b.name) == .orderedAscending
        }
    }

    // MARK: Lookup

    var total: Int { files.count }

    private static func same(_ a: String, _ b: String) -> Bool {
        a.compare(b, options: [.caseInsensitive, .diacriticInsensitive]) == .orderedSame
    }

    /// Library tags plus tags the library does not know yet (just created).
    private var candidates: [(name: String, files: Int)] {
        var list = library
        for file in files {
            for tag in file.tags where !list.contains(where: { Self.same($0.name, tag) }) { list.append((tag, 0)) }
        }
        return list
    }

    /// The spelling to use for `name`: the existing tag's, else the trimmed input.
    func canonical(_ name: String) -> String {
        let name = name.split(whereSeparator: \.isWhitespace).joined(separator: " ")
        return candidates.first { Self.same($0.name, name) }?.name ?? name
    }

    private func count(_ name: String) -> Int {
        files.filter { $0.tags.contains { Self.same($0, name) } }.count
    }

    func mark(_ name: String) -> TagMark {
        let n = count(name)
        if n == 0 || total == 0 { return .none }
        return n == total ? .all : .some(n)
    }

    /// Mixed-chip caption "2/3".
    func mixedCaption(_ mark: TagMark) -> String? {
        if case .some(let n) = mark { return "\(n)/\(total)" }
        return nil
    }

    // MARK: Lists

    /// Tags on at least one target, in the order they first appear.
    var chips: [Chip] {
        var seen: [String] = []
        for file in files {
            for tag in file.tags where !seen.contains(where: { Self.same($0, tag) }) { seen.append(tag) }
        }
        return seen.map { Chip(name: canonical($0), mark: mark($0)) }
    }

    /// The most used tags (library counts, most used first) with their marks.
    var mostUsed: [Row] {
        candidates.prefix(Self.mostUsedCount).map { .tag(name: $0.name, files: $0.files, mark: mark($0.name)) }
    }

    /// The list for the current text: "Most used" when the field is empty,
    /// else matches (prefix first, then contains; library order inside each
    /// tier) and the create row last when no tag equals the text.
    var rows: [Row] {
        let query = text.split(whereSeparator: \.isWhitespace).joined(separator: " ")
        guard !query.isEmpty else { return mostUsed }
        let opts: String.CompareOptions = [.caseInsensitive, .diacriticInsensitive]
        let all = candidates
        var result: [Row] = []
        for tier in 0...1 {
            for item in all {
                let prefix = item.name.range(of: query, options: opts.union(.anchored)) != nil
                let contains = prefix || item.name.range(of: query, options: opts) != nil
                if (tier == 0 && prefix) || (tier == 1 && contains && !prefix) {
                    result.append(.tag(name: item.name, files: item.files, mark: mark(item.name)))
                }
            }
        }
        if !all.contains(where: { Self.same($0.name, query) }) { result.append(.create(query)) }
        return result
    }

    /// The row the typed text means: the tag equal to it, else "Create tag".
    private var typedRowIndex: Int? {
        let list = rows
        return list.firstIndex {
            switch $0 {
            case .create: true
            case .tag(let name, _, _): Self.same(name, canonical(text))
            }
        }
    }

    mutating func moveHighlight(by delta: Int) {
        highlightMoved = true
        let count = rows.count
        guard count > 0 else { highlight = nil; return }
        guard let current = highlight else { highlight = delta > 0 ? 0 : count - 1; return }
        highlight = min(max(current + delta, 0), count - 1)
    }

    // MARK: Edits (each returns what to apply; the state is already updated)

    /// Adds the tag to every target that lacks it.
    @discardableResult
    mutating func add(_ name: String) -> TagChange? {
        let tag = canonical(name)
        guard !tag.isEmpty else { return nil }
        let ids = files.filter { f in !f.tags.contains { Self.same($0, tag) } }.map(\.id)
        guard !ids.isEmpty else { return nil }
        for i in files.indices where ids.contains(files[i].id) { files[i].tags.append(tag) }
        bump(tag, by: ids.count)
        return TagChange(kind: .add, tag: tag, fileIds: ids)
    }

    /// Removes the tag from every target that has it.
    @discardableResult
    mutating func remove(_ name: String) -> TagChange? {
        let tag = canonical(name)
        let ids = files.filter { f in f.tags.contains { Self.same($0, tag) } }.map(\.id)
        guard !ids.isEmpty else { return nil }
        for i in files.indices { files[i].tags.removeAll { Self.same($0, tag) } }
        bump(tag, by: -ids.count)
        return TagChange(kind: .remove, tag: tag, fileIds: ids)
    }

    /// Row / menu click: removes when all targets have it, else adds to all
    /// (a mixed tag becomes "all").
    @discardableResult
    mutating func toggle(_ name: String) -> TagChange? {
        mark(name) == .all ? remove(name) : add(name)
    }

    /// ⌫ on an empty field: removes the last chip from every target.
    @discardableResult
    mutating func removeLast() -> TagChange? {
        chips.last.flatMap { remove($0.name) }
    }

    /// Return: adds the typed tag (never removes: typing a tag the files
    /// already have is a no-op); after ↑/↓ toggles the picked row instead.
    /// Clears the text.
    @discardableResult
    mutating func activateHighlighted() -> [TagChange] {
        if !highlightMoved, !text.trimmingCharacters(in: .whitespaces).isEmpty {
            let change = add(text)
            text = ""
            return change.map { [$0] } ?? []
        }
        let list = rows
        guard let h = highlight, h < list.count else { return [] }
        let change: TagChange?
        switch list[h] {
        case .create(let name): change = add(name)
        case .tag(let name, _, _): change = toggle(name)
        }
        text = ""
        return change.map { [$0] } ?? []
    }

    /// Comma or Tab: the typed text becomes tags (comma separated, so a
    /// paste of "a, b, c" gives three).
    @discardableResult
    mutating func commitText() -> [TagChange] {
        let parts = text.split(separator: ",").map(String.init)
        text = ""
        return parts.compactMap { add($0) }
    }

    /// The field changed: complete comma-terminated pieces become tags and
    /// only the text after the last comma stays in the field.
    @discardableResult
    mutating func textChanged() -> [TagChange] {
        guard text.contains(",") else { return [] }
        var parts = text.split(separator: ",", omittingEmptySubsequences: false).map(String.init)
        let rest = parts.removeLast()
        let changes = parts.compactMap { add($0) }
        text = rest.trimmingCharacters(in: .whitespaces)
        return changes
    }

    /// Keeps the library counts in step with a live edit until the next reload.
    private mutating func bump(_ tag: String, by delta: Int) {
        if let i = library.firstIndex(where: { Self.same($0.name, tag) }) {
            library[i].files = max(library[i].files + delta, 0)
        } else if delta > 0 {
            library.append((tag, delta))
        }
    }
}
