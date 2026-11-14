import Foundation
import Observation

/// Non-modal, dismissible status message for watcher/indexing errors.
/// Repeats of the same text are coalesced into one banner with a counter, so
/// an error storm never stacks alerts.
@MainActor
@Observable
final class StatusBanner {
    /// The visible message, nil when dismissed.
    private(set) var message: String?
    /// How many times `message` was reported since it appeared.
    private(set) var count = 0

    /// Shows `text`, or bumps the counter when it is already shown.
    func post(_ text: String) {
        if message == text {
            count += 1
        } else {
            message = text
            count = 1
        }
    }

    func dismiss() {
        message = nil
        count = 0
    }

    /// Text for display, e.g. "Watcher failed (3 times)".
    var displayText: String? {
        guard let message else { return nil }
        return count > 1 ? "\(message) (\(count) times)" : message
    }
}
