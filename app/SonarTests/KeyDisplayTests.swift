import AppKit
import XCTest

@testable import Sonar

/// Key display (T10.6, ADR-041): tiers, en dash, source words, cents text,
/// and the "Include uncertain" wiring.
@MainActor
final class KeyDisplayTests: XCTestCase {
    func testTierBoundsAreInclusiveAndComeFromTheCore() {
        XCTAssertEqual(keyTierHigh(), 0.90)
        XCTAssertEqual(keyTierMedium(), 0.60)
        let high = keyTierHigh(), medium = keyTierMedium()
        XCTAssertEqual(Theme.KeyTier.of(confidence: high), .high)
        XCTAssertEqual(Theme.KeyTier.of(confidence: high.nextDown), .medium)
        XCTAssertEqual(Theme.KeyTier.of(confidence: medium), .medium)
        XCTAssertEqual(Theme.KeyTier.of(confidence: medium.nextDown), .low)
        XCTAssertEqual(Theme.KeyTier.of(confidence: 0), .low)
        XCTAssertEqual(Theme.KeyTier.of(confidence: nil), .low)
        XCTAssertEqual(Theme.KeyTier.of(confidence: .nan), .low)
        XCTAssertEqual(Theme.KeyTier.of(confidence: 1), .high)
    }

    func testTierConfidenceWords() {
        XCTAssertEqual([Theme.KeyTier.high, .medium, .low].map(\.word), ["High", "Medium", "Low"])
    }

    func testKeyLabelParts() {
        typealias P = KeyLabel.Parts
        XCTAssertEqual(KeyLabel.parts(key: "E", noteName: "E2", noteCents: 35), P(main: "E2", superscript: "+35"))
        XCTAssertEqual(KeyLabel.parts(key: "A#", noteName: "A#1", noteCents: -41.4), P(main: "A#1", superscript: "\u{2212}41"))
        XCTAssertEqual(KeyLabel.parts(key: "E", noteName: "E2", noteCents: 0.4), P(main: "E2", superscript: nil))
        XCTAssertEqual(KeyLabel.parts(key: "E", noteName: "E2", noteCents: -0.4), P(main: "E2", superscript: nil))
        XCTAssertEqual(KeyLabel.parts(key: "E", noteName: "E2", noteCents: nil), P(main: "E2", superscript: nil))
        XCTAssertEqual(KeyLabel.parts(key: "E", noteName: "E2", noteCents: .nan), P(main: "E2", superscript: nil))
        XCTAssertEqual(KeyLabel.parts(key: "D#m", noteName: nil, noteCents: 12), P(main: "D#m", superscript: nil))
        XCTAssertEqual(KeyLabel.parts(key: "C", noteName: "", noteCents: nil), P(main: "C", superscript: nil))
        XCTAssertNil(KeyLabel.parts(key: nil, noteName: nil, noteCents: nil))
        XCTAssertNil(KeyLabel.parts(key: "", noteName: nil, noteCents: nil))
        var row = SnapshotFixtures.rows(count: 1)[0]
        row.key = "Am"
        row.noteName = "E2"
        row.noteCents = 35
        row.keySource = "detected"
        XCTAssertEqual(KeyLabel.parts(row), P(main: "E2", superscript: "+35"))
        row.keySource = "user"
        XCTAssertEqual(KeyLabel.parts(row), P(main: "Am", superscript: nil), "an override hides the note")
        XCTAssertEqual(KeyLabel.plain(nil), "\u{2013}")
        XCTAssertEqual(KeyLabel.plain(P(main: "E2", superscript: "+35")), "E2 +35")
    }

    func testSourceAndConfidenceWords() {
        XCTAssertEqual(Format.keyDetail(source: "detected", tier: .high), "Audio · High")
        XCTAssertEqual(Format.keyDetail(source: "filename", tier: .low), "Name · Low")
        XCTAssertEqual(Format.keyDetail(source: "both", tier: .medium), "Audio + Name · Medium")
        XCTAssertEqual(Format.keyDetail(source: "user", tier: .high), "You · High")
        XCTAssertNil(Format.keyDetail(source: nil, tier: .low))
    }

    func testKeyCellRendersNoteWithSuperscriptInOneColour() {
        let cell = ValueCellView(identifier: NSUserInterfaceItemIdentifier("key"), value: .key)
        var row = SnapshotFixtures.rows(count: 1)[0]
        row.hidden = false
        row.key = "Am"
        row.noteName = nil
        row.noteCents = nil
        func colour(_ i: Int) -> NSColor? {
            cell.label.attributedStringValue.attribute(.foregroundColor, at: i, effectiveRange: nil) as? NSColor
        }
        for conf in [Float(0.95), 0.7, 0.2, 0] {
            row.keyConf = conf
            cell.configure(row, libraryName: nil)
            XCTAssertEqual(cell.label.stringValue, "Am")
            XCTAssertEqual(colour(0), .labelColor)
        }
        row.noteName = "E2"
        row.noteCents = 35
        cell.configure(row, libraryName: nil)
        XCTAssertEqual(cell.label.stringValue, "E2+35")
        XCTAssertEqual(colour(0), .labelColor)
        XCTAssertEqual(colour(2), .secondaryLabelColor)
        XCTAssertEqual(
            cell.label.attributedStringValue.attribute(.baselineOffset, at: 2, effectiveRange: nil) as? CGFloat,
            Theme.Size.superscriptBaseline)
        row.key = nil
        row.noteName = nil
        cell.configure(row, libraryName: nil)
        XCTAssertEqual(cell.label.stringValue, "\u{2013}")
        XCTAssertEqual(colour(0), .tertiaryLabelColor)
    }

    func testIncludeUncertainIsPassedToTheCore() {
        var state = FilterState()
        XCTAssertFalse(state.apply(to: SearchFilters()).includeUncertainKeys)
        state.includeUncertainKeys = true
        XCTAssertTrue(state.apply(to: SearchFilters()).includeUncertainKeys)
        XCTAssertTrue(state.isActive, "an active toggle counts as a filter (Clear all resets it)")
    }

    /// A detuned note stays on one line and the Key column cannot get
    /// narrower than it (founder screenshot 2026-11-19: `C#2` wrapped `−4`).
    func testKeyColumnFitsADetunedNoteOnOneLine() {
        let text = KeyLabel.attributed(KeyLabel.Parts(main: "G#4", superscript: "+50"))
        let style = text.attribute(.paragraphStyle, at: 0, effectiveRange: nil) as? NSParagraphStyle
        XCTAssertEqual(style?.lineBreakMode, .byTruncatingTail)
        let width = text.size().width
        let controller = ResultsTableController(pager: ResultsPager(pageSize: 10))
        let column = controller.tableView.tableColumn(withIdentifier: NSUserInterfaceItemIdentifier("key"))
        XCTAssertNotNil(column)
        XCTAssertGreaterThanOrEqual(column?.minWidth ?? 0, width + 2 * Theme.Space.s8)
        XCTAssertGreaterThanOrEqual(column?.width ?? 0, column?.minWidth ?? .infinity)
    }
}
