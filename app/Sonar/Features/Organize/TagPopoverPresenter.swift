import AppKit
import SwiftUI

/// Shows the tag popover anchored to the focused results row (⌘T, menus) as
/// a system `NSPopover`; the inspector `+` uses a SwiftUI popover instead.
@MainActor
final class TagPopoverPresenter: NSObject, NSPopoverDelegate {
    private var popover: NSPopover?
    private weak var model: AppModel?

    /// Reacts to `organizeState.tagPopover` changing.
    func sync(model: AppModel) {
        self.model = model
        guard let request = model.organizeState.tagPopover, request.anchor == .row else {
            close()
            return
        }
        guard popover == nil else { return }
        guard let anchor = model.keyboard.popoverAnchor?(request.ids.first) ?? nil else {
            model.organizeState.tagPopover = nil
            return
        }
        let pop = NSPopover()
        pop.behavior = .transient
        pop.delegate = self
        pop.contentViewController = TagPopoverController(model: model, ids: request.ids)
        popover = pop
        pop.show(relativeTo: anchor.rect, of: anchor.view, preferredEdge: .maxY)
    }

    private func close() {
        let pop = popover
        popover = nil
        pop?.delegate = nil
        pop?.close()
    }

    nonisolated func popoverDidClose(_ notification: Notification) {
        MainActor.assumeIsolated {
            popover = nil
            if model?.organizeState.tagPopover?.anchor == .row { model?.organizeState.tagPopover = nil }
        }
    }
}

/// Hosts the popover and routes ⌘Z / ⇧⌘Z to the window's undo manager, so a
/// live tag edit made in the popover undoes like any other (Edit > Undo).
@MainActor
final class TagPopoverController: NSHostingController<TagPopoverHost> {
    private weak var app: AppModel?

    init(model: AppModel, ids: [Int64]) {
        app = model
        super.init(rootView: TagPopoverHost(model: model, ids: ids))
    }

    @available(*, unavailable)
    @MainActor required dynamic init?(coder: NSCoder) { nil }

    override var undoManager: UndoManager? { app?.organizeState.undoManager ?? super.undoManager }
}
