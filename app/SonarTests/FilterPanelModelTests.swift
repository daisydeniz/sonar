import XCTest
@testable import Sonar

/// Selection model of the Filters panel and the bar's active chips (T11.1).
final class FilterPanelModelTests: XCTestCase {
    func testMultiSelectToggleAddsAndRemoves() {
        var f = FilterState()
        FilterState.toggle("kick", in: &f.categories)
        FilterState.toggle("bass", in: &f.categories)
        XCTAssertEqual(f.categories, ["kick", "bass"])
        FilterState.toggle("kick", in: &f.categories)
        XCTAssertEqual(f.categories, ["bass"])
    }

    func testQuickRangeIsSingleSelect() {
        var lo: Double?, hi: Double?
        let a = FilterPanel.tempoRanges[1], b = FilterPanel.tempoRanges[2]
        FilterPanel.toggleRange(a, min: &lo, max: &hi)
        XCTAssertEqual(FilterPanel.selectedRange(in: FilterPanel.tempoRanges, min: lo, max: hi), a)
        FilterPanel.toggleRange(b, min: &lo, max: &hi)
        XCTAssertEqual(FilterPanel.selectedRange(in: FilterPanel.tempoRanges, min: lo, max: hi), b)
        XCTAssertEqual([lo, hi], [110, 128])
        FilterPanel.toggleRange(b, min: &lo, max: &hi)
        XCTAssertNil(lo)
        XCTAssertNil(hi)
        // typing a different bound deselects the quick chip
        lo = 111
        hi = 128
        XCTAssertNil(FilterPanel.selectedRange(in: FilterPanel.tempoRanges, min: lo, max: hi))
        // open-ended ranges
        FilterPanel.toggleRange(FilterPanel.tempoRanges[0], min: &lo, max: &hi)
        XCTAssertNil(lo)
        XCTAssertEqual(hi, 90)
    }

    func testKeyModeMapsToKeys() {
        XCTAssertEqual(FilterPanel.keyLabels(root: "A", mode: .major), ["A"])
        XCTAssertEqual(FilterPanel.keyLabels(root: "A", mode: .minor), ["Am"])
        XCTAssertEqual(Set(FilterPanel.keyLabels(root: "C#", mode: .any)), ["C#", "C#m"])

        var keys = Set<String>()
        FilterPanel.toggleKey(root: "A", mode: .major, in: &keys)
        XCTAssertEqual(keys, ["A"])
        FilterPanel.toggleKey(root: "A", mode: .minor, in: &keys)
        XCTAssertEqual(keys, ["A", "Am"])
        FilterPanel.toggleKey(root: "A", mode: .any, in: &keys)  // selected in Any: removes both
        XCTAssertTrue(keys.isEmpty)
        FilterPanel.toggleKey(root: "D", mode: .any, in: &keys)
        XCTAssertEqual(keys, ["D", "Dm"])
        XCTAssertTrue(FilterPanel.isKeySelected(root: "D", mode: .minor, in: keys))
        XCTAssertFalse(FilterPanel.isKeySelected(root: "E", mode: .any, in: keys))
    }

    func testKeyCountsOnlyInMajorMinor() {
        let facets = [FacetCount(value: "A", count: 5), FacetCount(value: "Am", count: 3)]
        XCTAssertEqual(FilterPanel.keyCount(root: "A", mode: .major, facets: facets), 5)
        XCTAssertEqual(FilterPanel.keyCount(root: "A", mode: .minor, facets: facets), 3)
        XCTAssertNil(FilterPanel.keyCount(root: "A", mode: .any, facets: facets))
        XCTAssertEqual(FilterPanel.keyCount(root: "B", mode: .major, facets: facets), 0)
        XCTAssertTrue(FilterPanel.isKeyAvailable(root: "A", mode: .any, facets: facets, keys: []))
        XCTAssertFalse(FilterPanel.isKeyAvailable(root: "B", mode: .major, facets: facets, keys: []))
        XCTAssertTrue(FilterPanel.isKeyAvailable(root: "B", mode: .major, facets: facets, keys: ["B"]))
    }

    func testActiveChipsOnePerValueWithHues() {
        var f = FilterState()
        f.categories = ["bass", "kick"]
        f.keys = ["Am", "C"]
        f.bpmMin = 120
        f.bpmMax = 128
        f.durationMaxSec = 0.5
        f.tags = ["dark"]
        let chips = FilterPanel.activeChips(f)
        XCTAssertEqual(chips.map(\.title), ["Kick", "Bass", "C", "Am", "120–128 BPM", "≤ 0.5 s", "dark"])
        XCTAssertEqual(chips.map(\.hue), [.drum, .bass, .property, .property, .property, .property, .tag])

        f.keys = ["A", "Am"]
        XCTAssertEqual(FilterPanel.activeChips(f).filter { $0.hue == .property && $0.title.hasPrefix("A") }.map(\.title), ["A any"])
    }

    func testClearingAChipRemovesOnlyItsValue() {
        var f = FilterState()
        f.categories = ["kick", "bass"]
        f.keys = ["A", "Am"]
        f.compatibleKeys = true
        f.bpmMin = 90
        let chips = FilterPanel.activeChips(f)
        FilterPanel.clear(chips.first { $0.kind == .category("kick") }!, in: &f)
        XCTAssertEqual(f.categories, ["bass"])
        FilterPanel.clear(chips.first { $0.kind == .key(root: "A") }!, in: &f)
        XCTAssertTrue(f.keys.isEmpty)
        XCTAssertFalse(f.compatibleKeys)
        FilterPanel.clear(chips.first { $0.kind == .tempo }!, in: &f)
        XCTAssertNil(f.bpmMin)
    }

    func testShowMoreKeepsSelectedChipsVisible() {
        let chips = (0..<20).map { FacetChip(value: "t\($0)", count: 1, selected: $0 == 17) }
        XCTAssertEqual(FilterPanel.visible(chips, limit: 12, expanded: false).count, 13)
        XCTAssertEqual(FilterPanel.visible(chips, limit: 12, expanded: true).count, 20)
        XCTAssertEqual(FilterPanel.visible(Array(chips.prefix(5)), limit: 12, expanded: false).count, 5)
    }

    func testCategoriesGroupedByFamily() {
        let chips = ["vocal", "bass", "kick", "hat", "snare", "pad", "fx"].map {
            FacetChip(value: $0, count: 1, selected: false)
        }
        XCTAssertEqual(
            FilterPanel.categoryChips(chips).map(\.value), ["kick", "snare", "hat", "bass", "pad", "vocal", "fx"])
    }
}
