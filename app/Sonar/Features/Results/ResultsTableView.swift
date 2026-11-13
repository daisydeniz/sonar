import AppKit
import SwiftUI

/// SwiftUI host of the AppKit results table (`ResultsTableController`).
@MainActor
struct ResultsTableView: NSViewRepresentable {
    let pager: ResultsPager
    let libraryNames: [Int64: String]
    let onSelect: (FileRow?) -> Void
    let onSort: (SortKey, Bool) -> Void
    let onKey: (TableKey) -> Bool
    /// Called once with the controller so the model can focus the table.
    let register: (ResultsTableController) -> Void

    func makeCoordinator() -> ResultsTableController { ResultsTableController(pager: pager) }

    func makeNSView(context: Context) -> NSScrollView {
        let controller = context.coordinator
        controller.onSelect = onSelect
        controller.onSort = onSort
        controller.onKey = onKey
        controller.libraryNames = libraryNames
        register(controller)
        return controller.scrollView
    }

    func updateNSView(_ view: NSScrollView, context: Context) {
        let controller = context.coordinator
        controller.onSelect = onSelect
        controller.onSort = onSort
        controller.onKey = onKey
        controller.libraryNames = libraryNames
    }
}
