import SwiftUI

/// Duplicates: exact and near groups with path, size and format per file,
/// preview, reveal in Finder and "keep this one" (hides the rest; no file is
/// ever deleted).
@MainActor
struct DuplicatesView: View {
    let model: AppModel

    var body: some View {
        let dupes = model.duplicates
        Group {
            switch dupes.phase {
            case .idle, .loading:
                ProgressView("Looking for duplicates…").frame(maxWidth: .infinity, maxHeight: .infinity)
            case .failed(let message):
                EmptyStateView(
                    symbol: "exclamationmark.triangle", title: "Could not search for duplicates",
                    message: message)
            case .loaded:
                if dupes.groups.isEmpty {
                    EmptyStateView(
                        symbol: "checkmark.circle", title: "No duplicates",
                        message: dupes.nearFiles == 0
                            ? "No identical files. Near duplicates are found once indexing has finished."
                            : "No identical or near-identical samples found.")
                } else {
                    list(dupes)
                }
            }
        }
        .task { await model.runDuplicates() }
        .onDisappear { model.cancelDuplicates() }
        .toolbar {
            ToolbarItem {
                Button { Task { await model.runDuplicates() } } label: {
                    Label("Find Duplicates Again", systemImage: "arrow.clockwise")
                }
            }
        }
    }

    private func list(_ dupes: DuplicatesModel) -> some View {
        List(selection: Binding(
            get: { dupes.selection },
            set: { model.selectDuplicate(dupes.member($0), in: dupes.group(of: $0)) })
        ) {
            section("Exact duplicates", "Byte-identical files", dupes.exactGroups)
            section("Near duplicates", "The same sound in another format, rate or gain", dupes.nearGroups)
        }
        .onKeyPress(.space) {
            model.togglePreview()
            return .handled
        }
    }

    @ViewBuilder private func section(_ title: String, _ subtitle: String, _ groups: [DuplicateGroupItem]) -> some View {
        if !groups.isEmpty {
            Section {
                ForEach(groups) { group in
                    DuplicateGroupHeader(group: group)
                    ForEach(group.members) { member in
                        DuplicateRowView(model: model, group: group, member: member).tag(member.id)
                    }
                }
            } header: {
                Text("\(title) (\(groups.count) \(groups.count == 1 ? "group" : "groups")) - \(subtitle)")
            }
        }
    }
}

/// Group heading: kind label and the space keeping one file would free.
struct DuplicateGroupHeader: View {
    let group: DuplicateGroupItem

    var body: some View {
        HStack {
            StatusBadge(
                text: group.kind == .exact ? "Exact" : "Near",
                color: group.kind == .exact ? Theme.Color.error : Theme.Color.warning)
            Text("\(group.members.count) files").font(Theme.Font.body)
            Spacer()
            Text("Frees \(Format.bytes(group.wastedBytes))").font(Theme.Font.captionNumeric).foregroundStyle(.secondary)
        }
        .padding(.top, Theme.Space.s6)
        .accessibilityElement(children: .combine)
        .accessibilityLabel(
            "\(group.kind == .exact ? "Exact" : "Near") duplicate group, \(group.members.count) files, frees \(Format.bytes(group.wastedBytes))"
        )
    }
}

/// One file of a group.
struct DuplicateRowView: View {
    let model: AppModel
    let group: DuplicateGroupItem
    let member: DuplicateMember

    var body: some View {
        HStack(spacing: Theme.Space.s8) {
            VStack(alignment: .leading, spacing: Theme.Space.s2) {
                Text(member.row.name).font(Theme.Font.row).lineLimit(1)
                Text(member.row.path).font(Theme.Font.caption).foregroundStyle(.secondary)
                    .lineLimit(1).truncationMode(.middle)
            }
            Spacer()
            if member.row.hidden { Text("Hidden").font(Theme.Font.caption).foregroundStyle(.secondary) }
            Text(member.format).font(Theme.Font.caption).foregroundStyle(.secondary)
                .frame(width: 44, alignment: .trailing)
            Text(Format.bytes(member.size)).font(Theme.Font.captionNumeric).foregroundStyle(.secondary)
                .frame(width: 70, alignment: .trailing)
            Button { model.selectDuplicate(member, in: group) } label: { Image(systemName: "play.fill") }
                .help("Preview").accessibilityLabel("Preview \(member.row.name)")
            Button { model.revealDuplicate(member) } label: { Image(systemName: "folder") }
                .help("Reveal in Finder").accessibilityLabel("Reveal \(member.row.name) in Finder")
            Button("Keep This") { Task { await model.keepOnly(member, in: group) } }
                .help("Keep this file, hide the others in the group (nothing is deleted)")
            Button(member.row.hidden ? "Unhide" : "Hide") {
                Task { await model.setDuplicateHidden(member, on: !member.row.hidden) }
            }
        }
        .buttonStyle(.borderless)
        .opacity(member.row.hidden ? 0.5 : 1)
        .accessibilityElement(children: .contain)
    }
}
