import SwiftUI
import UniformTypeIdentifiers

/// Welcome screen shown while there is no library: pitch, "Add Folders…"
/// and drop folders from Finder.
@MainActor
struct OnboardingView: View {
    let model: AppModel
    @State private var targeted = false

    var body: some View {
        VStack(spacing: 14) {
            Image(systemName: "waveform.circle").font(.system(size: 56)).foregroundStyle(.tint)
                .accessibilityHidden(true)
            Text("Welcome to Sonar").font(.largeTitle.bold())
            Text("Find the right sample fast. Everything runs 100% on your Mac, offline: your audio never leaves it.")
                .font(.title3).foregroundStyle(.secondary).multilineTextAlignment(.center)
                .frame(maxWidth: 460)
            Button("Add Folders…") {
                let paths = FolderPicker.pickFolders(prompt: "Add")
                if !paths.isEmpty { Task { await model.addLibraries(paths: paths) } }
            }
            .controlSize(.large).buttonStyle(.borderedProminent)
            .keyboardShortcut(.defaultAction)
            Text("or drop folders here").font(.callout).foregroundStyle(.tertiary)
        }
        .padding(32)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .overlay {
            if targeted {
                RoundedRectangle(cornerRadius: 10).stroke(Color.accentColor, lineWidth: 3).padding(8)
                    .allowsHitTesting(false)
            }
        }
        .onDrop(of: [.fileURL], isTargeted: $targeted) { providers in
            let model = model
            Task { @MainActor in
                var paths: [String] = []
                for provider in providers {
                    if let url = await Self.loadURL(provider), Self.isFolder(url) { paths.append(url.path) }
                }
                await model.addLibraries(paths: paths)
            }
            return true
        }
        .accessibilityElement(children: .contain)
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
        ContentUnavailableView {
            Label("Could not open Sonar's library", systemImage: "exclamationmark.triangle")
        } description: {
            Text(message).textSelection(.enabled)
        } actions: {
            Button("Try Again") { Task { await model.retryOpen() } }
                .buttonStyle(.borderedProminent)
            Button("Reveal Data Folder") {
                NSWorkspace.shared.activateFileViewerSelecting([AppSettings.dataFolder()])
            }
        }
    }
}
