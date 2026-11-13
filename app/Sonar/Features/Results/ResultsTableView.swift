import SwiftUI

/// Simple SwiftUI table of the first results page. Isolated here because
/// T6.2 replaces it with a paged NSTableView.
@MainActor
struct ResultsTableView: View {
    let rows: [FileRow]
    @Binding var selection: Int64?

    var body: some View {
        Table(rows, selection: $selection) {
            TableColumn("Name", value: \.name)
            TableColumn("Category") { Text(Format.text($0.category)) }
            TableColumn("Kind") { Text(Format.text($0.kind)) }
            TableColumn("BPM") { Text(Format.bpm($0.bpm)).monospacedDigit() }
                .width(60)
            TableColumn("Key") { Text(Format.text($0.key)) }
                .width(60)
            TableColumn("Duration") { Text(Format.duration(ms: $0.durationMs)).monospacedDigit() }
                .width(80)
        }
        .accessibilityLabel("Search results")
    }
}
