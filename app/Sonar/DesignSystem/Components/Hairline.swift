import SwiftUI

/// A 1 px separator in the `hairline` colour (DESIGN §2.1).
struct Hairline: View {
    enum Axis { case horizontal, vertical }
    var axis: Axis = .horizontal

    var body: some View {
        switch axis {
        case .horizontal:
            Theme.Color.hairline.frame(height: Theme.hairlineWidth)
        case .vertical:
            Theme.Color.hairline.frame(width: Theme.hairlineWidth)
        }
    }
}
