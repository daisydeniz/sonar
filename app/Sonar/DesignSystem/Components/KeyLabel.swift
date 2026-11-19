import AppKit
import SwiftUI

/// Key / note rendering shared by the results table, the KEY readout and
/// the inspector (DESIGN §4.1, ADR-041 addendum T10.8): a pitched one-shot
/// shows its note with octave and the rounded cents as a small superscript
/// ("E2" + "+35"); anything else shows its key ("D#m"); nothing shows an
/// en dash in tertiary. One colour for every value (no tier colouring).
enum KeyLabel {
    /// Text of the missing-value dash.
    static let dash = "\u{2013}"

    /// What to draw: main text and an optional superscript.
    struct Parts: Equatable {
        let main: String
        let superscript: String?
    }

    /// Pure formatter: nil when there is neither a note nor a key.
    /// The superscript is the signed rounded cents ("+35", "\u{2212}12"),
    /// omitted when it rounds to 0 or the cents are missing / not finite.
    static func parts(key: String?, noteName: String?, noteCents: Float?) -> Parts? {
        if let noteName, !noteName.isEmpty {
            guard let noteCents, noteCents.isFinite else { return Parts(main: noteName, superscript: nil) }
            let ct = Int(noteCents.rounded())
            guard ct != 0 else { return Parts(main: noteName, superscript: nil) }
            let sign = ct < 0 ? "\u{2212}" : "+"
            return Parts(main: noteName, superscript: "\(sign)\(abs(ct))")
        }
        if let key, !key.isEmpty { return Parts(main: key, superscript: nil) }
        return nil
    }

    /// Parts of a result row. A user key override hides the note (the core
    /// keeps the note fields, but the user's key is what counts).
    static func parts(_ row: FileRow) -> Parts? {
        let overridden = row.keySource == "user"
        return parts(
            key: row.key, noteName: overridden ? nil : row.noteName,
            noteCents: overridden ? nil : row.noteCents)
    }

    /// Plain text ("E2 +35", "Am", en dash) for accessibility and tests.
    static func plain(_ parts: Parts?) -> String {
        guard let parts else { return dash }
        return parts.superscript.map { "\(parts.main) \($0)" } ?? parts.main
    }

    /// AppKit rendering for the table: main text in `color`, superscript
    /// smaller, raised and in `superscriptColor`; en dash in tertiary.
    static func attributed(
        _ parts: Parts?, font: NSFont = Theme.NSFonts.row, color: NSColor = .labelColor,
        superscriptColor: NSColor = .secondaryLabelColor, missingColor: NSColor = .tertiaryLabelColor
    ) -> NSAttributedString {
        // One line, cut at the end: without a paragraph style an attributed
        // label wraps the superscript onto a second line in a narrow column.
        let paragraph = NSMutableParagraphStyle()
        paragraph.lineBreakMode = .byTruncatingTail
        guard let parts else {
            return NSAttributedString(string: dash, attributes: [
                .font: font, .foregroundColor: missingColor, .paragraphStyle: paragraph,
            ])
        }
        let result = NSMutableAttributedString(
            string: parts.main, attributes: [.font: font, .foregroundColor: color, .paragraphStyle: paragraph])
        if let sup = parts.superscript {
            result.append(NSAttributedString(string: sup, attributes: [
                .font: Theme.NSFonts.superscript, .foregroundColor: superscriptColor,
                .baselineOffset: Theme.Size.superscriptBaseline, .paragraphStyle: paragraph,
            ]))
        }
        return result
    }

    /// Width of the widest value (`G#4` + `+50`, `A#4` + `−45`, `C#m`) in `font`, rounded
    /// up: the results table's Key column never gets narrower than this
    /// plus its cell insets, so a detuned note always fits on one line.
    static func widestWidth(font: NSFont = Theme.NSFonts.row) -> CGFloat {
        let samples = [
            Parts(main: "G#4", superscript: "+50"), Parts(main: "A#4", superscript: "\u{2212}45"),
            Parts(main: "C#m", superscript: nil),
        ]
        return samples.map { attributed($0, font: font).size().width }.max().map { $0.rounded(.up) } ?? 0
    }

    /// SwiftUI rendering as one `Text` (so it truncates and aligns like text).
    static func text(_ parts: Parts?, font: SwiftUI.Font = Theme.Font.row) -> Text {
        guard let parts else { return Text(dash).font(font).foregroundStyle(.tertiary) }
        let main = Text(parts.main).font(font).foregroundStyle(.primary)
        guard let sup = parts.superscript else { return main }
        return main + Text(sup).font(Theme.Font.superscript).baselineOffset(Theme.Size.superscriptBaseline)
            .foregroundStyle(.secondary)
    }
}

/// SwiftUI view of a row's key / note.
struct KeyLabelView: View {
    let parts: KeyLabel.Parts?
    var font: Font = Theme.Font.row

    var body: some View {
        KeyLabel.text(parts, font: font)
            .lineLimit(1)
            .accessibilityLabel(KeyLabel.plain(parts))
    }
}
