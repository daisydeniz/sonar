import SwiftUI

/// Inspector card (DESIGN §3): opaque `surfaceGroup`, radius `large`, 10/12
/// padding, with a `caption` semibold secondary title and an optional
/// trailing accessory (e.g. the Tags `+` button) in the title row.
struct InspectorCard<Accessory: View, Content: View>: View {
    let title: String
    @ViewBuilder let accessory: Accessory
    @ViewBuilder let content: Content

    init(
        _ title: String, @ViewBuilder accessory: () -> Accessory,
        @ViewBuilder content: () -> Content
    ) {
        self.title = title
        self.accessory = accessory()
        self.content = content()
    }

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.s2) {
            HStack(spacing: Theme.Space.s4) {
                Text(title)
                    .font(Theme.Font.caption.weight(.semibold)).foregroundStyle(.secondary)
                    .accessibilityAddTraits(.isHeader)
                Spacer(minLength: 0)
                accessory
            }
            content
        }
        .padding(.horizontal, Theme.Space.s12)
        .padding(.vertical, Theme.Size.cardPaddingVertical)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Theme.Color.surfaceGroup, in: RoundedRectangle(cornerRadius: Theme.Radius.large))
        .accessibilityElement(children: .contain)
    }
}

extension InspectorCard where Accessory == EmptyView {
    /// Card without a title-row accessory.
    init(_ title: String, @ViewBuilder content: () -> Content) {
        self.init(title, accessory: { EmptyView() }, content: content)
    }
}
