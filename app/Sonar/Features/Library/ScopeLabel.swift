import Foundation

/// Window title and subtitle of the main toolbar (DESIGN §3): the title is
/// the current scope ("All Samples", a library, a collection), the subtitle
/// the libraries in that scope. Pure functions so they can be tested.
enum ScopeLabel {
    /// At most this many library names are listed; the rest is "+ n more".
    static let maxNames = 3
    private static let separator = " \u{00B7} "

    static func title(
        _ selection: SidebarItem, libraries: [(id: Int64, name: String)],
        collections: [(id: Int64, name: String)]
    ) -> String {
        switch selection {
        case .allSamples: "All Samples"
        case .favorites: "Favorites"
        case .duplicates: "Duplicates"
        case .library(let id): libraries.first { $0.id == id }?.name ?? "Library"
        case .collection(let id): collections.first { $0.id == id }?.name ?? "Collection"
        }
    }

    /// Libraries in scope, `·`-separated; empty when there are none (the
    /// toolbar then shows no subtitle). A single library as scope has no
    /// subtitle of its own (the title already names it).
    static func subtitle(_ selection: SidebarItem, libraries: [(id: Int64, name: String)]) -> String {
        if case .library = selection { return "" }
        let names = libraries.map(\.name)
        guard names.count > maxNames else { return names.joined(separator: separator) }
        let shown = names.prefix(maxNames).joined(separator: separator)
        return "\(shown) + \(names.count - maxNames) more"
    }
}

extension AppModel {
    /// Library (id, folder name) pairs as the scope helpers want them.
    private var scopeLibraries: [(id: Int64, name: String)] {
        libraries.map { ($0.id, URL(fileURLWithPath: $0.rootPath).lastPathComponent) }
    }

    var scopeTitle: String {
        ScopeLabel.title(
            sidebarSelection, libraries: scopeLibraries,
            collections: collections.map { ($0.id, $0.name) })
    }

    var scopeSubtitle: String { ScopeLabel.subtitle(sidebarSelection, libraries: scopeLibraries) }
}
