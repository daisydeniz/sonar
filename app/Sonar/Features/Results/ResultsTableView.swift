import AppKit
import SwiftUI

/// SwiftUI host of the AppKit results table (`ResultsTableController`).
@MainActor
struct ResultsTableView: NSViewRepresentable {
    let pager: ResultsPager
    let libraryNames: [Int64: String]
    let onSelect: (FileRow?) -> Void
    let onSort: (SortKey, Bool) -> Void

    func makeCoordinator() -> ResultsTableController { ResultsTableController(pager: pager) }

    func makeNSView(context: Context) -> NSScrollView {
        let controller = context.coordinator
        controller.onSelect = onSelect
        controller.onSort = onSort
        controller.libraryNames = libraryNames
        return controller.scrollView
    }

    func updateNSView(_ view: NSScrollView, context: Context) {
        let controller = context.coordinator
        controller.onSelect = onSelect
        controller.onSort = onSort
        controller.libraryNames = libraryNames
    }
}
