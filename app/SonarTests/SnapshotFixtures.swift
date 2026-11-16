import Foundation
@testable import Sonar

/// Deterministic sample data for UI snapshots (no engine, no audio).
enum SnapshotFixtures {
    static let libraries = [
        LibraryInfo(id: 1, rootPath: "/Samples/KSHMR Vol 1", online: true, addedAt: 1_700_000_000),
        LibraryInfo(id: 2, rootPath: "/Volumes/Archive/Drum Hits", online: false, addedAt: 1_700_100_000),
    ]
    static let collections = [
        CollectionInfo(id: 1, name: "Techno Kit", files: 24),
        CollectionInfo(id: 2, name: "Vocal Chops", files: 12),
        CollectionInfo(id: 3, name: "Favorites 2026", files: 58),
    ]

    private static let categories = [
        "kick", "snare", "clap", "tom", "hat", "cymbal", "perc", "bass",
        "lead", "pad", "keys", "pluck", "synth", "vocal", "fx", "texture", "drums",
    ]
    private static let keys = ["Am", "C", "F#m", "Dm", "G", "A#", "Em", "C#m"]
    private static let words = ["Punchy", "Dark", "Warm", "Tight", "Dusty", "Wide", "Heavy", "Crisp", "Soft", "Hard"]
    private static let packs = ["KSHMR", "Cymatics", "Vengeance", "Splice", "Loopmasters"]

    /// `count` rows, ids 1...count; mix of loops and one-shots.
    static func rows(count: Int = 200) -> [FileRow] {
        (1...count).map { i in
            let category = categories[i % categories.count]
            let loop = ["bass", "pad", "keys", "lead", "drums", "texture", "synth"].contains(category) && i % 3 != 0
            let word = words[(i * 7) % words.count]
            let pack = packs[i % packs.count]
            let name = "\(pack)_\(category.capitalized)_\(word)_\(String(format: "%02d", i % 40 + 1))"
            let libraryId: Int64 = i % 5 == 0 ? 2 : 1
            let root = libraryId == 1 ? "/Samples/KSHMR Vol 1" : "/Volumes/Archive/Drum Hits"
            let folder = "\(category.capitalized)s"
            return FileRow(
                fileId: Int64(i), libraryId: libraryId, path: "\(root)/\(folder)/\(name).wav", name: name,
                folders: "\(URL(fileURLWithPath: root).lastPathComponent)/\(folder)", category: category,
                kind: loop ? "loop" : "one_shot", bpm: loop ? Double(90 + (i * 3) % 60) : (i % 11 == 0 ? 128 : nil),
                key: i % 4 == 0 ? nil : keys[i % keys.count],
                durationMs: loop ? UInt64(3000 + (i * 137) % 5000) : UInt64(120 + (i * 53) % 1800),
                sampleRate: 44100, channels: i % 3 == 0 ? 1 : 2, favorite: i % 17 == 0, hidden: i == 9 || i == 33,
                tags: i % 4 == 0 ? ["dark", "club"] : (i % 5 == 0 ? ["analog"] : []))
        }
    }

    /// Facet counts computed from `rows`.
    static func facets(of rows: [FileRow]) -> SearchFacets {
        func counts(_ values: [String]) -> [FacetCount] {
            Dictionary(grouping: values, by: { $0 }).map { FacetCount(value: $0.key, count: UInt64($0.value.count)) }
                .sorted { ($0.count, $1.value) > ($1.count, $0.value) }
        }
        return SearchFacets(
            categories: counts(rows.compactMap(\.category)), kinds: counts(rows.compactMap(\.kind)),
            keys: counts(rows.compactMap(\.key)),
            libraries: [
                LibraryFacet(libraryId: 1, count: UInt64(rows.filter { $0.libraryId == 1 }.count)),
                LibraryFacet(libraryId: 2, count: UInt64(rows.filter { $0.libraryId == 2 }.count)),
            ],
            tags: counts(rows.flatMap(\.tags)), favorites: UInt64(rows.filter(\.favorite).count))
    }

    /// Smooth decaying waveform with some texture.
    static func peaks(count: Int = 512) -> [WaveformPeak] {
        (0..<count).map { i in
            let t = Float(i) / Float(count)
            let env = exp(-3 * t) * (0.55 + 0.45 * abs(sin(Float(i) * 0.37)))
            return WaveformPeak(min: -env, max: env * 0.95)
        }
    }

    /// Duplicate groups over the first rows.
    static func duplicates(rows: [FileRow]) -> DuplicatesResult {
        DuplicatesResult(
            groups: [
                DuplicateGroup(
                    kind: .exact, fileIds: [1, 2, 3], sizes: [880_000, 880_000, 880_000], wastedBytes: 1_760_000,
                    maxDistance: 0, meanDistance: 0),
                DuplicateGroup(
                    kind: .exact, fileIds: [4, 5], sizes: [420_000, 420_000], wastedBytes: 420_000,
                    maxDistance: 0, meanDistance: 0),
                DuplicateGroup(
                    kind: .near, fileIds: [6, 7, 8], sizes: [300_000, 150_000, 150_000], wastedBytes: 300_000,
                    maxDistance: 0.04, meanDistance: 0.03),
            ],
            files: UInt64(rows.count), hashed: UInt64(rows.count), skipped: 0, nearFiles: UInt64(rows.count),
            threshold: 0.05)
    }
}

/// Page loader returning fixed rows with facets on page 0.
struct SnapshotPageLoader: ResultPageLoader {
    let rows: [FileRow]
    func loadPage(offset: Int, limit: Int) async throws -> PageResult {
        let start = min(offset, rows.count)
        let end = min(start + limit, rows.count)
        return PageResult(
            rows: Array(rows[start..<end]), total: rows.count, hasMore: end < rows.count,
            facets: offset == 0 ? SnapshotFixtures.facets(of: rows) : nil)
    }
}

/// Fixed peaks for any file.
struct SnapshotWaveformProvider: WaveformProviding {
    func peaks(fileId: Int64, buckets: Int) async throws -> [WaveformPeak] { SnapshotFixtures.peaks() }
}

/// License backend that always fails (the manager then fails open).
struct SnapshotLicenseService: LicenseService {
    struct Unavailable: Error {}
    func status() async throws -> LicenseState { throw Unavailable() }
    func activate(key: String) async throws -> LicenseState { throw Unavailable() }
    func deactivate() async throws -> LicenseState { throw Unavailable() }
}

/// Page 0 loads, every other page never arrives (skeleton rows stay).
struct StalledPageLoader: ResultPageLoader {
    let rows: [FileRow]
    func loadPage(offset: Int, limit: Int) async throws -> PageResult {
        if offset > 0 { try await Task.sleep(for: .seconds(3600)) }
        return PageResult(rows: Array(rows.prefix(limit)), total: rows.count, hasMore: true, facets: nil)
    }
}
