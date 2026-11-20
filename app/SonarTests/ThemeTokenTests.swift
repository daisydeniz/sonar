import AppKit
import SwiftUI
import XCTest
@testable import Sonar

/// T12.1: v2 colour sets exist, adapt to appearance and meet WCAG AA
/// (docs/DESIGN.md §2.1, ADR-043).
@MainActor
final class ThemeTokenTests: XCTestCase {
    private typealias RGBA = [CGFloat]
    private let white: RGBA = [1, 1, 1, 1]

    /// Sets that must differ between light and dark.
    private let adaptive = [
        "SurfaceContent", "SurfaceRaised", "SurfaceGroup", "WaveWell", "WaveStrip", "Wave", "Hairline", "Favorite",
        "AccentColor", "AccentContent", "Signal", "KnobNeutral",
        "FacetCategory", "FacetKey", "FacetProperty", "FacetTag", "FacetLibrary",
    ]
    /// Sets that are the same in both appearances.
    private let fixed = [
        "ChipBase", "KnobPitch", "KnobGain", "KnobFilter", "KnobTime",
        "FacetCategoryFill", "FacetKeyFill", "FacetPropertyFill", "FacetTagFill", "FacetLibraryFill",
    ]

    private func rgba(_ color: NSColor, dark: Bool) -> RGBA {
        var out: RGBA = []
        NSAppearance(named: dark ? .darkAqua : .aqua)!.performAsCurrentDrawingAppearance {
            let c = color.usingColorSpace(.sRGB)!
            out = [c.redComponent, c.greenComponent, c.blueComponent, c.alphaComponent]
        }
        return out
    }

    private func luminance(_ c: RGBA) -> Double {
        func lin(_ v: CGFloat) -> Double {
            let x = Double(v)
            return x <= 0.03928 ? x / 12.92 : pow((x + 0.055) / 1.055, 2.4)
        }
        return 0.2126 * lin(c[0]) + 0.7152 * lin(c[1]) + 0.0722 * lin(c[2])
    }

    private func contrast(_ a: RGBA, _ b: RGBA) -> Double {
        let (l1, l2) = (luminance(a), luminance(b))
        return (max(l1, l2) + 0.05) / (min(l1, l2) + 0.05)
    }

    /// `top` at `alpha` composited over opaque `bottom`.
    private func over(_ top: RGBA, alpha: CGFloat, _ bottom: RGBA) -> RGBA {
        (0..<3).map { top[$0] * alpha + bottom[$0] * (1 - alpha) } + [1]
    }

    func testEveryColorSetLoads() throws {
        for name in adaptive + fixed {
            XCTAssertNotNil(NSColor(named: NSColor.Name(name), bundle: .main), name)
        }
        for name in adaptive {
            let color = try XCTUnwrap(NSColor(named: NSColor.Name(name), bundle: .main), name)
            XCTAssertNotEqual(rgba(color, dark: false), rgba(color, dark: true), name)
        }
    }

    func testDerivedColorsResolvePerAppearance() {
        XCTAssertEqual(rgba(Theme.NS.selection, dark: false)[3], 0.12, accuracy: 0.01)
        XCTAssertEqual(rgba(Theme.NS.selection, dark: true)[3], 0.20, accuracy: 0.01)
        XCTAssertEqual(rgba(Theme.NS.selectionInactive, dark: false)[3], 0.08, accuracy: 0.01)
        XCTAssertEqual(rgba(Theme.NS.signalSoft, dark: false)[3], 0.18, accuracy: 0.01)
        XCTAssertEqual(rgba(Theme.NS.signalSoft, dark: true)[3], 0.16, accuracy: 0.01)
        XCTAssertEqual(rgba(Theme.NS.waveGrid, dark: true)[3], 0.14, accuracy: 0.01)
        XCTAssertEqual(rgba(Theme.NS.waveCentre, dark: false)[3], 0.28, accuracy: 0.01)
        XCTAssertNotEqual(rgba(Theme.NS.signalSoft, dark: false), rgba(Theme.NS.signalSoft, dark: true))
    }

    /// System controls draw white text on the `AccentColor` asset.
    func testWhiteTextOnControlAccent() {
        for dark in [false, true] {
            let ratio = contrast(white, rgba(Theme.NS.controlAccent, dark: dark))
            XCTAssertGreaterThanOrEqual(ratio, 4.5, "dark=\(dark)")
        }
    }

    /// Selected chips: white text on every hue's deep fill.
    func testWhiteTextOnEveryHueFill() {
        XCTAssertEqual(Theme.Hue.allCases.count, 5)
        for hue in Theme.Hue.allCases {
            for dark in [false, true] {
                let ratio = contrast(white, rgba(hue.nsFill, dark: dark))
                XCTAssertGreaterThanOrEqual(ratio, 4.5, "\(hue.fillAsset) dark=\(dark)")
            }
        }
    }

    /// Unselected chips: hue text on its chip background (light: fill at 11 %
    /// over the surface the chip sits on; dark: `chipBase`) and on `surfaceGroup`.
    func testHueTextContrast() {
        for hue in Theme.Hue.allCases {
            for dark in [false, true] {
                let text = rgba(hue.nsColor, dark: dark)
                let group = rgba(Theme.NS.surfaceGroup, dark: dark)
                let content = rgba(Theme.NS.surfaceContent, dark: dark)
                var backgrounds: [(String, RGBA)] = [("surfaceGroup", group)]
                if dark {
                    backgrounds.append(("chipBase", rgba(Theme.NS.chipBase, dark: true)))
                } else {
                    let fill = rgba(hue.nsFill, dark: false)
                    backgrounds.append(("tint/group", over(fill, alpha: 0.11, group)))
                    backgrounds.append(("tint/content", over(fill, alpha: 0.11, content)))
                    backgrounds.append(("tintHover/content", over(fill, alpha: 0.18, content)))
                }
                for (name, bg) in backgrounds {
                    let ratio = contrast(text, bg)
                    XCTAssertGreaterThanOrEqual(ratio, 4.5, "\(hue.textAsset) on \(name) dark=\(dark): \(ratio)")
                }
            }
        }
    }

    /// Accent and signal text are readable on the content surfaces.
    func testAccentAndSignalContrast() {
        for dark in [false, true] {
            for token in [Theme.NS.accent, Theme.NS.signal] {
                for surface in [Theme.NS.surfaceContent, Theme.NS.surfaceRaised] {
                    let ratio = contrast(rgba(token, dark: dark), rgba(surface, dark: dark))
                    XCTAssertGreaterThanOrEqual(ratio, 4.5, "dark=\(dark)")
                }
            }
        }
    }

    func testScales() {
        XCTAssertEqual([Theme.Space.s2, Theme.Space.s4, Theme.Space.s6, Theme.Space.s8, Theme.Space.s12,
                        Theme.Space.s16, Theme.Space.s24, Theme.Space.s32], [2, 4, 6, 8, 12, 16, 24, 32])
        XCTAssertEqual([Theme.Radius.small, Theme.Radius.medium, Theme.Radius.well, Theme.Radius.large], [4, 6, 8, 10])
        XCTAssertEqual([Theme.Size.rowHeight, Theme.Size.headerHeight, Theme.Size.barHeight], [26, 24, 44])
        XCTAssertEqual([Theme.Size.chipHeight, Theme.Size.chipHeightCompact], [24, 22])
        XCTAssertEqual([Theme.Size.knobSmall, Theme.Size.knobLarge], [24, 44])
        XCTAssertEqual(Theme.hairlineWidth(scale: 2), 0.5)
    }
}
