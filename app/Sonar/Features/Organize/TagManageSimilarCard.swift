import SwiftUI

/// The "n similar tags — may be typos" card of the Manage Tags sheet
/// (DESIGN §4.12): on `signalSoft`, each pair as two chips with their counts,
/// the reason, "Merge into “x”" (the more used spelling wins) and an
/// icon-only Ignore. Shown only when the core suggests any pair.
@MainActor
struct TagManageSimilarCard: View {
    let model: TagManageModel

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.s8) {
            HStack(spacing: Theme.Space.s8) {
                Image(systemName: "square.on.square").foregroundStyle(Theme.Color.signal)
                Text("\(model.pairs.count) similar \(model.pairs.count == 1 ? "tag" : "tags") — may be typos")
                    .font(Theme.Font.body.weight(.semibold))
                Spacer(minLength: 0)
                Button("Merge All") { Task { await model.mergeAll() } }
                    .buttonStyle(.link).font(Theme.Font.body)
                    .disabled(model.isBusy)
                    .help("Merge every pair into its more used spelling")
            }
            ScrollView {
                VStack(alignment: .leading, spacing: Theme.Space.s8) {
                    ForEach(model.pairs, id: \.self) { pairRow($0) }
                }
            }
            .frame(maxHeight: Theme.Size.manageTagsPairsMaxHeight)
            .fixedSize(horizontal: false, vertical: true)
            Text("Case and spaces merge automatically: “Club ” = “club”.")
                .font(Theme.Font.caption).foregroundStyle(.secondary)
        }
        .padding(Theme.Space.s12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Theme.Color.signalSoft, in: RoundedRectangle(cornerRadius: Theme.Radius.large))
        .accessibilityElement(children: .contain)
    }

    private func pairRow(_ pair: SimilarTagPair) -> some View {
        let keep = TagManageLogic.survivor(of: pair).keep
        return HStack(spacing: Theme.Space.s8) {
            SonarChip(title: pair.a, count: Int(pair.aFiles), hue: .tag, action: {}).allowsHitTesting(false)
            Text("·").foregroundStyle(.tertiary)
            SonarChip(title: pair.b, count: Int(pair.bFiles), hue: .tag, action: {}).allowsHitTesting(false)
            Text(TagManageLogic.reasonText(pair.reason))
                .font(Theme.Font.caption).foregroundStyle(.tertiary)
            Spacer(minLength: Theme.Space.s8)
            Button("Merge into “\(keep)”") { Task { await model.merge(pair) } }
                .controlSize(.small)
                .disabled(model.isBusy)
            Button { Task { await model.ignore(pair) } } label: { Image(systemName: "xmark") }
                .buttonStyle(.borderless).controlSize(.small)
                .disabled(model.isBusy)
                .help("Not the same tag: don't suggest this pair again")
                .accessibilityLabel("Ignore \(pair.a) and \(pair.b)")
        }
        .accessibilityElement(children: .contain)
    }
}
