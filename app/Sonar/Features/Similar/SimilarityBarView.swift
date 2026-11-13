import SwiftUI

/// "Similar to: <name>" bar above the results: the five aspect sliders,
/// Reset weights, a progress indicator and inline status (no alerts).
@MainActor
struct SimilarityBarView: View {
    let model: AppModel

    var body: some View {
        let similarity = model.similarity
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 8) {
                Image(systemName: "waveform.badge.magnifyingglass").foregroundStyle(.secondary)
                Text("Similar to: \(similarity.target?.displayName ?? "")")
                    .font(.headline).lineLimit(1).truncationMode(.middle)
                if similarity.isLoading {
                    ProgressView().controlSize(.small).accessibilityLabel("Finding similar samples")
                }
                if let message = similarity.message {
                    Text(message).font(.callout).foregroundStyle(.orange).lineLimit(1)
                }
                Spacer()
                Button("Reset Weights") { model.resetSimilarityWeights() }
                    .disabled(similarity.sliders == AspectSliders())
                Button { model.exitSimilarity() } label: { Image(systemName: "xmark.circle.fill") }
                    .buttonStyle(.borderless)
                    .keyboardShortcut(.cancelAction)
                    .help("Back to search (Esc)")
                    .accessibilityLabel("Close similarity search")
            }
            HStack(spacing: 14) {
                ForEach(SimilarityAspect.allCases, id: \.self) { aspect in
                    VStack(alignment: .leading, spacing: 0) {
                        Text(aspect.title).font(.caption).foregroundStyle(.secondary)
                        Slider(
                            value: Binding(
                                get: { Double(similarity.sliders[aspect]) },
                                set: { model.setSimilarityWeight(aspect, Float($0)) }),
                            in: 0...1
                        )
                        .controlSize(.small)
                        .accessibilityLabel("\(aspect.title) weight")
                    }
                }
            }
        }
        .padding(.horizontal, 12).padding(.vertical, 8)
        .background(.bar)
    }
}
