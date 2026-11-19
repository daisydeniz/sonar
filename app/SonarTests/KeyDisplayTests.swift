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

    func testTierTextColours() {
        XCTAssertEqual(Theme.KeyTier.high.nsColor, .labelColor)
        XCTAssertEqual(Theme.KeyTier.medium.nsColor, .secondaryLabelColor)
        XCTAssertEqual(Theme.KeyTier.low.nsColor, .tertiaryLabelColor)
        XCTAssertEqual([Theme.KeyTier.high, .medium, .low].map(\.word), ["High", "Medium", "Low"])
    }

    func testEnDashWhenThereIsNoKey() {
        XCTAssertEqual(Format.keyText(nil), "\u{2013}")
        XCTAssertEqual(Format.keyText(""), "\u{2013}")
        XCTAssertEqual(Format.keyText("Am"), "Am")
    }

    func testSourceAndConfidenceWords() {
        XCTAssertEqual(Format.keyDetail(source: "detected", tier: .high), "Audio · High")
        XCTAssertEqual(Format.keyDetail(source: "filename", tier: .low), "Name · Low")
        XCTAssertEqual(Format.keyDetail(source: "both", tier: .medium), "Audio + Name · Medium")
        XCTAssertEqual(Format.keyDetail(source: "user", tier: .high), "You · High")
        XCTAssertNil(Format.keyDetail(source: nil, tier: .low))
    }

    func testNoteWithCents() {
        XCTAssertEqual(Format.note(name: "E2", cents: 32), "≈E2 +32 ct")
        XCTAssertEqual(Format.note(name: "E2", cents: 25), "E2 +25 ct", "25 itself is not past 25")
        XCTAssertEqual(Format.note(name: "E2", cents: 12.4), "E2 +12 ct")
        XCTAssertEqual(Format.note(name: "A#1", cents: -41), "≈A#1 -41 ct")
        XCTAssertEqual(Format.note(name: "C4", cents: 0), "C4 +0 ct")
        XCTAssertEqual(Format.note(name: "C4", cents: nil), "C4")
        XCTAssertNil(Format.note(name: nil, cents: 10))
    }

    func testKeyCellUsesTierColourAndEnDash() {
        let cell = ValueCellView(identifier: NSUserInterfaceItemIdentifier("key"), value: .key)
        var row = SnapshotFixtures.rows(count: 1)[0]
        row.hidden = false
        row.key = "Am"
        for (conf, tier) in [(Float(0.95), Theme.KeyTier.high), (0.7, .medium), (0.2, .low), (0, .low)] {
            row.keyConf = conf
            cell.configure(row, libraryName: nil)
            XCTAssertEqual(cell.label.stringValue, "Am")
            XCTAssertEqual(cell.label.textColor, tier.nsColor)
        }
        row.key = nil
        cell.configure(row, libraryName: nil)
        XCTAssertEqual(cell.label.stringValue, "\u{2013}")
        XCTAssertEqual(cell.label.textColor, .tertiaryLabelColor)
    }

    func testIncludeUncertainIsPassedToTheCore() {
        var state = FilterState()
        XCTAssertFalse(state.apply(to: SearchFilters()).includeUncertainKeys)
        state.includeUncertainKeys = true
        XCTAssertTrue(state.apply(to: SearchFilters()).includeUncertainKeys)
        XCTAssertTrue(state.isActive, "an active toggle counts as a filter (Clear all resets it)")
    }
}
