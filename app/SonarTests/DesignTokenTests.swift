import AppKit
import SwiftUI
import XCTest
@testable import Sonar

/// T9.1: colour sets exist, differ between appearances, categories map, and
/// the accent is readable on the content surfaces (WCAG AA).
@MainActor
final class DesignTokenTests: XCTestCase {
    private let names = [
        "SurfaceContent", "SurfaceRaised", "SurfaceWell", "Hairline", "Favorite",
        "CatDrum", "CatMetal", "CatBass", "CatTonal", "CatVocal", "CatFX", "AccentColor", "AccentContent",
        "FacetProperty", "FacetTag",
    ]

    private func rgba(_ color: NSColor, dark: Bool) -> [CGFloat] {
        var out: [CGFloat] = []
        NSAppearance(named: dark ? .darkAqua : .aqua)!.performAsCurrentDrawingAppearance {
            let c = color.usingColorSpace(.sRGB)!
            out = [c.redComponent, c.greenComponent, c.blueComponent, c.alphaComponent]
        }
        return out
    }

    private func luminance(_ c: [CGFloat]) -> Double {
        func lin(_ v: CGFloat) -> Double {
            let x = Double(v)
            return x <= 0.03928 ? x / 12.92 : pow((x + 0.055) / 1.055, 2.4)
        }
        return 0.2126 * lin(c[0]) + 0.7152 * lin(c[1]) + 0.0722 * lin(c[2])
    }

    private func contrast(_ a: [CGFloat], _ b: [CGFloat]) -> Double {
        let (l1, l2) = (luminance(a), luminance(b))
        return (max(l1, l2) + 0.05) / (min(l1, l2) + 0.05)
    }

    func testEveryColorSetLoadsAndDiffersBetweenAppearances() throws {
        for name in names {
            let color = try XCTUnwrap(NSColor(named: NSColor.Name(name), bundle: .main), name)
            XCTAssertNotEqual(rgba(color, dark: false), rgba(color, dark: true), name)
        }
    }

    func testDerivedColorsResolvePerAppearance() {
        XCTAssertEqual(rgba(Theme.NS.selection, dark: false)[3], 0.16, accuracy: 0.01)
        XCTAssertEqual(rgba(Theme.NS.selection, dark: true)[3], 0.24, accuracy: 0.01)
        XCTAssertEqual(rgba(Theme.NS.accentSoft, dark: true)[3], 0.14, accuracy: 0.01)
        XCTAssertEqual(rgba(Theme.NS.selectionInactive, dark: false)[3], 0.08, accuracy: 0.01)
        XCTAssertNotEqual(rgba(Theme.NS.accentSoft, dark: false), rgba(Theme.NS.accentSoft, dark: true))
    }

    func testCategoryMapping() {
        let groups: [(NSColor, [String])] = [
            (Theme.NS.catDrum, ["kick", "snare", "clap", "tom", "drums"]),
            (Theme.NS.catMetal, ["hat", "cymbal", "perc"]),
            (Theme.NS.catBass, ["bass"]),
            (Theme.NS.catTonal, ["lead", "pad", "keys", "pluck", "synth"]),
            (Theme.NS.catVocal, ["vocal"]),
            (Theme.NS.catFX, ["fx", "texture", "something_unknown"]),
        ]
        for (expected, categories) in groups {
            for c in categories {
                let got = Theme.categoryNSColor(c)
                XCTAssertNotNil(got, c)
                XCTAssertEqual(rgba(got!, dark: false), rgba(expected, dark: false), c)
                XCTAssertEqual(rgba(got!, dark: true), rgba(expected, dark: true), c)
                XCTAssertNotNil(Theme.categoryColor(c), c)
            }
        }
        XCTAssertNil(Theme.categoryColor(nil))
        XCTAssertNil(Theme.categoryNSColor(nil))
    }

    func testAccentContrastOnSurfaces() {
        for dark in [false, true] {
            let accent = rgba(Theme.NS.accent, dark: dark)
            for surface in [Theme.NS.surfaceContent, Theme.NS.surfaceRaised] {
                XCTAssertGreaterThanOrEqual(contrast(accent, rgba(surface, dark: dark)), 4.5, "dark=\(dark)")
            }
        }
    }

    /// System controls draw white text on the `AccentColor` asset.
    func testWhiteTextOnControlAccent() {
        for dark in [false, true] {
            let fill = rgba(Theme.NS.controlAccent, dark: dark)
            XCTAssertGreaterThanOrEqual(contrast([1, 1, 1, 1], fill), 4.5, "dark=\(dark)")
        }
    }

    /// Selected hued chips: white text on every hue's deep fill, both appearances (ADR-042).
    func testWhiteTextOnEveryHueFill() {
        XCTAssertEqual(Theme.Hue.allCases.count, 8)
        for hue in Theme.Hue.allCases {
            for dark in [false, true] {
                let fill = rgba(hue.nsFill, dark: dark)
                XCTAssertGreaterThanOrEqual(contrast([1, 1, 1, 1], fill), 4.5, "\(hue.fillAsset) dark=\(dark)")
            }
        }
    }

    func testHueColoursAndCategoryMapping() {
        for hue in Theme.Hue.allCases {
            let color = NSColor(named: NSColor.Name(hue.textAsset), bundle: .main)!
            XCTAssertNotEqual(rgba(color, dark: false), rgba(color, dark: true), hue.textAsset)
        }
        XCTAssertEqual(Theme.categoryHue("snare"), .drum)
        XCTAssertEqual(Theme.categoryHue("texture"), .fx)
        XCTAssertNil(Theme.categoryHue(nil))
    }

    func testScales() {
        XCTAssertEqual([Theme.Space.s2, Theme.Space.s4, Theme.Space.s6, Theme.Space.s8, Theme.Space.s12,
                        Theme.Space.s16, Theme.Space.s24, Theme.Space.s32], [2, 4, 6, 8, 12, 16, 24, 32])
        XCTAssertEqual(Theme.Size.rowHeight, 24)
        XCTAssertEqual([Theme.Radius.small, Theme.Radius.medium, Theme.Radius.large], [4, 6, 10])
        XCTAssertEqual([Theme.Size.headerHeight, Theme.Size.barHeight, Theme.Size.waveformHeight,
                        Theme.Size.transportButton, Theme.Size.chipHeight], [22, 36, 180, 28, 22])
        XCTAssertEqual(Theme.hairlineWidth(scale: 2), 0.5)
    }
}
