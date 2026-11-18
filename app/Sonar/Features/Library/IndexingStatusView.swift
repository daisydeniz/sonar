import SwiftUI

/// Toolbar status: indexing progress with Pause / Resume.
@MainActor
struct IndexingStatusView: View {
    let model: AppModel

    var body: some View {
        let progress = model.progress
        if progress.running {
            HStack(spacing: Theme.Space.s8) {
                ProgressView(value: Double(progress.done), total: Double(max(progress.total, 1)))
                    .controlSize(.small)
                    .frame(width: Theme.Size.indexingProgressWidth)
                    .accessibilityLabel("Indexing progress")
                Text(label(progress))
                    .font(Theme.Font.captionNumeric)
                    .foregroundStyle(.secondary)
                if progress.paused {
                    Button { model.resumeIndexing() } label: { Image(systemName: "play.fill") }
                        .help("Resume indexing")
                        .accessibilityLabel("Resume indexing")
                } else {
                    Button { model.pauseIndexing() } label: { Image(systemName: "pause.fill") }
                        .help("Pause indexing")
                        .accessibilityLabel("Pause indexing")
                }
            }
            .padding(.horizontal, Theme.Space.s8)
        }
    }

    private func label(_ p: IndexingProgress) -> String {
        let stage: String
        switch p.stage {
        case .meta: stage = "Reading"
        case .dsp: stage = "Analyzing"
        case .embed: stage = "Embedding"
        case nil: stage = "Indexing"
        }
        return "\(p.paused ? "Paused: " : "")\(stage) \(p.done)/\(p.total)"
    }
}
