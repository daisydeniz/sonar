import Foundation

extension AppModel {
    /// Handles one engine event; returns whether libraries/results should be
    /// refreshed. Errors go to the non-modal banner (never an alert).
    func apply(_ event: EngineEvent) -> Bool {
        switch event {
        case .libraryScanned, .libraryOffline, .libraryOnline, .indexRefreshed, .indexingFinished:
            return true
        case .error(let message):
            banner.post(message)
            return false
        case .indexingStarted:
            return false
        }
    }
}
