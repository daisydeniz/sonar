import Foundation

/// Provider-agnostic licensing backend (ADR-038). The UI only talks to this
/// protocol; the Rust core decides what a valid key is.
protocol LicenseService: Sendable {
    /// Current trial/license state (the first call starts the trial).
    func status() async throws -> LicenseState
    /// Verifies and stores `key`; an invalid key throws `FfiError.License`.
    func activate(key: String) async throws -> LicenseState
    /// Removes the stored key and returns the resulting state.
    func deactivate() async throws -> LicenseState
}

/// `LicenseService` backed by the core's free FFI functions. They work even
/// when the engine failed to open. `home == nil` lets the core pick its data
/// folder (`SONAR_HOME`, else Application Support), exactly like the engine.
struct CoreLicenseService: LicenseService {
    var home: String?

    init(home: String? = nil) { self.home = home }

    func status() async throws -> LicenseState {
        let home = home
        return try await Task.detached { try licenseStatus(home: home) }.value
    }

    func activate(key: String) async throws -> LicenseState {
        let home = home
        return try await Task.detached { try activateLicense(home: home, key: key) }.value
    }

    func deactivate() async throws -> LicenseState {
        let home = home
        return try await Task.detached { try deactivateLicense(home: home) }.value
    }
}
