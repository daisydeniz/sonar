import SwiftUI
import UniformTypeIdentifiers

/// Welcome screen shown while there is no library: pitch, "Add Folders…"
/// and drop folders from Finder.
@MainActor
struct OnboardingView: View {
    let model: AppModel
    @State private var targeted = false

    var body: some View {
        EmptyStateView(
            symbol: "waveform", title: "Welcome to Sonar",
            message: "Find the right sample fast. Everything runs 100% on your Mac, offline: your audio never leaves it.",
            actionTitle: "Add Folders…",
            action: {
                let paths = FolderPicker.pickFolders(prompt: "Add")
                if !paths.isEmpty { Task { await model.addLibraries(paths: paths) } }
            },
            extra: { Text("or drop folders here").font(Theme.Font.caption).foregroundStyle(.secondary) }
        )
        .defaultActionShortcut()
        .overlay {
            if targeted {
                DropOverlayView(message: "Drop folders to add them as libraries", symbol: "folder.badge.plus")
            }
        }
        .onDrop(of: [.fileURL], isTargeted: $targeted) { providers in
            let model = model
            Task { @MainActor in
                var paths: [String] = []
                for provider in providers {
                    if let url = await Self.loadURL(provider), Self.isFolder(url) { paths.append(url.path) }
                }
                if paths.isEmpty {
                    model.banner.post("Drop folders, not files, to add libraries.")
                } else {
                    await model.addLibraries(paths: paths)
                }
            }
            return true
        }
    }

    private static func isFolder(_ url: URL) -> Bool {
        (try? url.resourceValues(forKeys: [.isDirectoryKey]))?.isDirectory == true
    }

    private static func loadURL(_ provider: NSItemProvider) async -> URL? {
        await withCheckedContinuation { continuation in
            _ = provider.loadObject(ofClass: URL.self) { url, _ in continuation.resume(returning: url) }
        }
    }
}

/// Full-window error when the engine could not open.
@MainActor
struct EngineFailedView: View {
    let model: AppModel
    let message: String

    var body: some View {
        EmptyStateView(
            symbol: "exclamationmark.triangle", title: "Could not open Sonar's library", message: message,
            actionTitle: "Try Again", action: { Task { await model.retryOpen() } },
            extra: {
                Button("Reveal Data Folder") {
                    NSWorkspace.shared.activateFileViewerSelecting([AppSettings.revealableDataFolder()])
                }
            })
    }
}
