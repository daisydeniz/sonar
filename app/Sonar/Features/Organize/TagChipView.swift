import SwiftUI

/// A tag of the selection as a `facetTag` chip with `×` (DESIGN §4.12): on
/// all selected files it is a plain chip (click or × removes it); on only
/// some it is the dashed "n/m" chip (click adds it to all, × removes it
/// from all). Shared by the inspector Tags card and the tag popover.
struct TagChipView: View {
    let chip: TagPopoverState.Chip
    /// "2/3" for a mixed chip.
    let mixedCaption: String?
    let onAdd: () -> Void
    let onRemove: () -> Void

    var body: some View {
        if let mixedCaption {
            SonarChip(
                title: chip.name, hue: .tag, mixed: mixedCaption, onClear: onRemove,
                clearLabel: "Remove from all", action: onAdd)
                .help("On some of the selected samples. Click to add to all.")
                .accessibilityLabel("\(chip.name), on \(mixedCaption) samples. Add to all")
        } else {
            SonarChip(title: chip.name, hue: .tag, trailingSymbol: "xmark", action: onRemove)
                .help("Remove tag")
                .accessibilityLabel("Remove tag \(chip.name)")
        }
    }
}
