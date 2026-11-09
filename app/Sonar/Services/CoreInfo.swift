import Foundation

/// Display-only facts about the linked Rust core (`sonar-ffi`, via the UniFFI
/// bindings in `app/Generated/SonarCore.swift`).
///
/// Threading: `coreVersion()` only formats a compile-time constant, so calling
/// it on the main actor is fine. Real core work (scan, analysis, search) must
/// run off the main thread on background tasks, with latest-query-wins
/// cancellation; that bridge (`Services/CoreBridge`, ARCHITECTURE.md §8)
/// arrives with the first real FFI calls.
enum CoreInfo {
    /// Semantic version of the Rust core library, e.g. "0.1.0". Read once.
    static let version: String = coreVersion()
}
