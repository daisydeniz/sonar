import AppKit
import SwiftUI
import XCTest
@testable import Sonar

@MainActor
final class PlayerHeaderTests: XCTestCase {
    func testLanesHeightIsClampedTo96And320() {
        XCTAssertEqual(PlayerHeaderLayout.clamped(180), 180)
        XCTAssertEqual(PlayerHeaderLayout.clamped(10), 96)
        XCTAssertEqual(PlayerHeaderLayout.clamped(1000), 320)
        XCTAssertEqual(PlayerHeaderLayout.clamped(.nan), Theme.Size.waveformHeight)
        XCTAssertEqual(Theme.Size.waveformHeight, 150)
    }

    /// Without a selection the header renders (flat centre line, disabled
    /// transport) and takes transport row + ruler + lanes + readout strip + divider height.
    func testHeaderRendersWithoutSelection() throws {
        let defaults = try XCTUnwrap(UserDefaults(suiteName: "player-header-\(UUID().uuidString)"))
        let preview = AudioPreview(player: FakePlayer(), defaults: defaults)
        let host = NSHostingView(
            rootView: PlayerHeaderView(row: nil, preview: preview, waveform: WaveformModel()).frame(width: 900))
        let fitting = host.fittingSize
        XCTAssertGreaterThanOrEqual(
            fitting.height,
            Theme.Size.transportRowHeight + Theme.Size.rulerHeight + Theme.Size.waveformMinHeight
                + Theme.Size.readoutStripHeight + Theme.Size.splitHandle)
        XCTAssertEqual(WaveformModel().state, .empty)
    }
}
