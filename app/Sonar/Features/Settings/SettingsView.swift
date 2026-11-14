import SwiftUI

/// Settings window (⌘,): Libraries, Preview, Indexing, About.
@MainActor
struct SettingsView: View {
    let model: AppModel

    var body: some View {
        TabView {
            LibrariesSettingsView(model: model)
                .tabItem { Label("Libraries", systemImage: "folder") }
            PreviewSettingsView(preview: model.preview)
                .tabItem { Label("Preview", systemImage: "speaker.wave.2") }
            IndexingSettingsView(model: model)
                .tabItem { Label("Indexing", systemImage: "gearshape.2") }
            AboutSettingsView()
                .tabItem { Label("About", systemImage: "info.circle") }
        }
        .frame(width: 560, height: 380)
    }
}

/// Preview toggles; both live on `AudioPreview`, which persists them.
@MainActor
struct PreviewSettingsView: View {
    @Bindable var preview: AudioPreview

    var body: some View {
        Form {
            Toggle("Auto-play on selection", isOn: $preview.autoPlay)
            Toggle("Loudness-normalized preview", isOn: $preview.normalizeLoudness)
            Text("Normalized preview plays quiet and loud samples at a similar level. Files on disk are never changed.")
                .font(.caption).foregroundStyle(.secondary)
        }
        .formStyle(.grouped)
    }
}

/// Indexing options and pause/resume.
@MainActor
struct IndexingSettingsView: View {
    let model: AppModel

    var body: some View {
        let progress = model.progress
        Form {
            Toggle("Natural-language analysis (CLAP)", isOn: Bindable(model.settings).includeEmbeddings)
            Text("Lets you search by description (\"warm analog bass\"). Takes more time while indexing. Applies the next time indexing starts.")
                .font(.caption).foregroundStyle(.secondary)
            LabeledContent("Status") {
                Text(progress.running ? (progress.paused ? "Paused" : "Indexing \(progress.done)/\(progress.total)") : "Idle")
            }
            if progress.running {
                if progress.paused {
                    Button("Resume Indexing") { model.resumeIndexing() }
                } else {
                    Button("Pause Indexing") { model.pauseIndexing() }
                }
            }
        }
        .formStyle(.grouped)
    }
}

/// App and core version plus third-party notices.
struct AboutSettingsView: View {
    static let notices: [(name: String, detail: String)] = [
        ("LAION CLAP (Apache-2.0)", "Converted to ONNX and int8-quantized. Wu et al., 2023."),
        ("ONNX Runtime (MIT)", "Runs the CLAP model on-device."),
        ("Tantivy (MIT), SQLite (public domain), Symphonia (MPL-2.0)", "Search index, store and audio decoding."),
    ]

    var body: some View {
        Form {
            LabeledContent("Sonar", value: AppInfo.versionString())
            LabeledContent("Core", value: CoreInfo.version)
            Section("Third-party notices") {
                ForEach(Self.notices, id: \.name) { notice in
                    VStack(alignment: .leading, spacing: 2) {
                        Text(notice.name)
                        Text(notice.detail).font(.caption).foregroundStyle(.secondary)
                    }
                    .accessibilityElement(children: .combine)
                }
            }
            Text("Sonar runs 100% locally. Your audio and index never leave your Mac.")
                .font(.caption).foregroundStyle(.secondary)
        }
        .formStyle(.grouped)
    }
}
