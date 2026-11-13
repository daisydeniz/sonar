import AppKit
import SwiftUI
import UniformTypeIdentifiers

/// Which dropped files start a similarity search.
enum SimilarityDrop {
    static let audioExtensions: Set<String> = ["wav", "aif", "aiff", "flac", "mp3", "ogg", "m4a"]

    /// The first file URL with an audio extension (multi-file drops use it).
    static func firstAudioURL(in urls: [URL]) -> URL? {
        urls.first { $0.isFileURL && audioExtensions.contains($0.pathExtension.lowercased()) }
    }
}

/// Drop target of the results area: accepts Finder audio files only (the
/// drag pasteboard is read synchronously so other drags get no highlight).
@MainActor
struct SimilarityDropDelegate: DropDelegate {
    let model: AppModel
    @Binding var isTargeted: Bool

    private func draggedURLs() -> [URL] {
        let objects = NSPasteboard(name: .drag).readObjects(
            forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true])
        return (objects as? [URL]) ?? []
    }

    func validateDrop(info: DropInfo) -> Bool {
        let urls = draggedURLs()
        if urls.isEmpty { return info.hasItemsConforming(to: [.fileURL]) }
        return SimilarityDrop.firstAudioURL(in: urls) != nil
    }

    func dropEntered(info: DropInfo) { isTargeted = true }
    func dropExited(info: DropInfo) { isTargeted = false }

    func performDrop(info: DropInfo) -> Bool {
        isTargeted = false
        let providers = info.itemProviders(for: [.fileURL])
        guard !providers.isEmpty else { return false }
        let model = model
        Task { @MainActor in
            var urls: [URL] = []
            for provider in providers {
                if let url = await Self.loadURL(provider) { urls.append(url) }
            }
            if let first = SimilarityDrop.firstAudioURL(in: urls) { model.findSimilar(externalFile: first) }
        }
        return true
    }

    private static func loadURL(_ provider: NSItemProvider) async -> URL? {
        await withCheckedContinuation { continuation in
            _ = provider.loadObject(ofClass: URL.self) { url, _ in continuation.resume(returning: url) }
        }
    }
}
