import Foundation
import Observation

/// Persisted app settings that are not owned by another object. (Preview
/// toggles live on `AudioPreview`, which persists them itself.)
///
/// `defaults` is injectable so tests use a throwaway suite, never `.standard`.
@MainActor
@Observable
final class AppSettings {
    static let embeddingsKey = "indexing.includeEmbeddings"

    /// Natural-language analysis (CLAP) while indexing. Applies to the next
    /// indexing start (library add or app launch). Default on.
    var includeEmbeddings: Bool {
        didSet { defaults.set(includeEmbeddings, forKey: Self.embeddingsKey) }
    }

    @ObservationIgnored private let defaults: UserDefaults

    init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
        includeEmbeddings = defaults.object(forKey: Self.embeddingsKey) as? Bool ?? true
    }

    /// Where the core keeps its data: `SONAR_HOME`, else
    /// `~/Library/Application Support/Sonar` (same rule as the core).
    nonisolated static func dataFolder(
        environment: [String: String] = ProcessInfo.processInfo.environment
    ) -> URL {
        if let home = environment["SONAR_HOME"], !home.isEmpty { return URL(fileURLWithPath: home) }
        return FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Library/Application Support/Sonar")
    }
}
