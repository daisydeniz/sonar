import SwiftUI

/// Similarity bar (DESIGN §3): one 36 pt row on `surfaceRaised` with the
/// target name (and region seed), the five compact aspect sliders, a progress
/// indicator / inline status, and icon-only Reset and Close. No alerts.
@MainActor
struct SimilarityBarView: View {
    let model: AppModel

    var body: some View {
        let similarity = model.similarity
        HStack(spacing: Theme.Space.s8) {
            Image(systemName: "waveform.badge.magnifyingglass")
                .imageScale(.small).foregroundStyle(.secondary).accessibilityHidden(true)
            Text(title(similarity))
                .font(Theme.Font.row).lineLimit(1).truncationMode(.middle)
                .layoutPriority(1)
            if similarity.isLoading {
                ProgressView().controlSize(.small).accessibilityLabel("Finding similar samples")
            }
            if let message = similarity.message {
                Text(message).font(Theme.Font.caption).foregroundStyle(Theme.Color.warning).lineLimit(1)
            }
            Spacer(minLength: 0)
            ForEach(SimilarityAspect.allCases, id: \.self) { aspect in
                HStack(spacing: Theme.Space.s4) {
                    Text(aspect.title).font(Theme.Font.caption).foregroundStyle(.secondary).fixedSize()
                    Slider(
                        value: Binding(
                            get: { Double(similarity.sliders[aspect]) },
                            set: { model.setSimilarityWeight(aspect, Float($0)) }),
                        in: 0...1
                    )
                    .controlSize(.small)
                    .frame(minWidth: Theme.Space.s16, maxWidth: Theme.Size.aspectSliderWidth)
                    .accessibilityLabel("\(aspect.title) weight")
                }
            }
            Button { model.resetSimilarityWeights() } label: { Image(systemName: "arrow.counterclockwise") }
                .buttonStyle(.borderless)
                .disabled(similarity.sliders == AspectSliders())
                .help("Reset weights")
                .accessibilityLabel("Reset weights")
            Button { model.exitSimilarity() } label: { Image(systemName: "xmark") }
                .buttonStyle(.borderless)
                .keyboardShortcut(.cancelAction)
                .help("Back to search (Esc)")
                .accessibilityLabel("Close similarity search")
        }
        .controlSize(.small)
        .padding(.horizontal, Theme.Space.s12)
        .frame(height: Theme.Size.barHeight)
        .background(Theme.Color.surfaceRaised)
    }

    private func title(_ similarity: SimilarityModel) -> String {
        let name = similarity.target?.displayName ?? ""
        guard let region = similarity.region else { return "Similar to: \(name)" }
        return "Similar to: \(name) (\(String(format: "%.2f", region.startS))–\(String(format: "%.2f", region.endS)) s)"
    }
}
