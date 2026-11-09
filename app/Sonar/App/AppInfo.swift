import Foundation

/// Static, display-only facts about the running app bundle.
enum AppInfo {
    /// Product name shown in the window title and placeholder UI.
    static let name = "Sonar"

    /// Marketing version from the bundle, e.g. "0.1.0 (1)".
    static func versionString(bundle: Bundle = .main) -> String {
        versionString(info: bundle.infoDictionary ?? [:])
    }

    /// Formats version keys from an Info.plist dictionary; missing keys fall
    /// back to "0.0.0 (0)".
    static func versionString(info: [String: Any]) -> String {
        let short = info["CFBundleShortVersionString"] as? String ?? "0.0.0"
        let build = info["CFBundleVersion"] as? String ?? "0"
        return "\(short) (\(build))"
    }
}
