import AppKit
import SwiftUI
import XCTest
@testable import Sonar

@MainActor
final class PlayerHeaderTests: XCTestCase {
    func testHeightIsClampedTo120And360() {
        XCTAssertEqual(PlayerHeaderLayout.clamped(180), 180)
        XCTAssertEqual(PlayerHeaderLayout.clamped(10), 120)
        XCTAssertEqual(PlayerHeaderLayout.clamped(1000), 360)
        XCTAssertEqual(PlayerHeaderLayout.clamped(.nan), Theme.Size.waveformHeight)
        XCTAssertEqual(Theme.Size.waveformHeight, 180)
    }

    /// Without a selection the header renders (flat centre line, disabled
    /// transport) and takes transport row + waveform + divider height.
    func testHeaderRendersWithoutSelection() throws {
        let defaults = try XCTUnwrap(UserDefaults(suiteName: "player-header-\(UUID().uuidString)"))
        let preview = AudioPreview(player: FakePlayer(), defaults: defaults)
        let host = NSHostingView(
            rootView: PlayerHeaderView(row: nil, preview: preview, waveform: WaveformModel()).frame(width: 900))
        let fitting = host.fittingSize
        XCTAssertGreaterThanOrEqual(
            fitting.height, Theme.Size.barHeight + Theme.Size.waveformMinHeight + Theme.Size.splitHandle)
        XCTAssertEqual(WaveformModel().state, .empty)
    }
}
