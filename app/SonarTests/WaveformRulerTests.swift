import XCTest
@testable import Sonar

final class WaveformRulerTests: XCTestCase {
    private func labels(_ ticks: [RulerTick]) -> [String] { ticks.compactMap(\.label) }

    func testBarsWithBpmWholeFile() {
        // 120 BPM, 8 s = 4 bars, 100 pt per second: beats are 50 pt (< 60), bars 200 pt.
        let ticks = WaveformRuler.ticks(duration: 8, bpm: 120, viewport: WaveformViewport(), width: 800)
        XCTAssertEqual(labels(ticks), ["1", "2", "3", "4", "5"])
        XCTAssertEqual(ticks.count, 65)  // sixteenth ticks (12.5 pt apart)
        XCTAssertEqual(ticks.first?.fraction, 0)
        XCTAssertEqual(ticks[4].fraction, 0.5 / 8, accuracy: 1e-9)  // first beat
        XCTAssertTrue(ticks[4].major)
        XCTAssertFalse(ticks[1].major)
    }

    func testBeatLabelsWhenZoomed() {
        let ticks = WaveformRuler.ticks(
            duration: 8, bpm: 120, viewport: WaveformViewport(zoom: 2, start: 0), width: 800)
        XCTAssertEqual(Array(labels(ticks).prefix(6)), ["1", "1.2", "1.3", "1.4", "2", "2.2"])
        XCTAssertLessThanOrEqual(ticks.last?.fraction ?? 2, 0.5 + 1e-9)  // only the visible half
    }

    func testBarLabelsThinOutWhenNarrow() {
        // 15 pt per second: bars are 30 pt apart, so every second bar is labelled.
        let ticks = WaveformRuler.ticks(duration: 8, bpm: 120, viewport: WaveformViewport(), width: 120)
        XCTAssertEqual(labels(ticks), ["1", "3", "5"])
        XCTAssertEqual(ticks.count, 5)  // only bar ticks fit
    }

    func testSecondsWithoutBpm() {
        let ticks = WaveformRuler.ticks(duration: 2, bpm: nil, viewport: WaveformViewport(), width: 600)
        XCTAssertEqual(Array(labels(ticks).prefix(3)), ["0.0", "0.2", "0.4"])
        XCTAssertEqual(labels(ticks).last, "2.0")
        XCTAssertEqual(ticks.count, 41)  // 0.05 s minor ticks
        // Labels are at least 60 pt apart.
        let xs = ticks.filter { $0.label != nil }.map { $0.fraction * 600 }
        for (a, b) in zip(xs, xs.dropFirst()) { XCTAssertGreaterThanOrEqual(b - a, 60 - 1e-6) }
        XCTAssertEqual(WaveformRuler.secondStep(pxPerSec: 120), 0.5)
        XCTAssertEqual(WaveformRuler.secondStep(pxPerSec: 100), 1)
    }

    func testSecondsFollowZoomAndPan() {
        let v = WaveformViewport(zoom: 4, start: 0.5)  // 2.0 s ... 3.0 s of 4 s
        let ticks = WaveformRuler.ticks(duration: 4, bpm: nil, viewport: v, width: 600)
        XCTAssertEqual(labels(ticks).first, "2.0")
        XCTAssertEqual(labels(ticks).last, "3.0")
        XCTAssertEqual(labels(ticks)[1], "2.1")
        for t in ticks { XCTAssertTrue((0.5 - 1e-9)...(0.75 + 1e-9) ~= t.fraction) }
    }

    func testBpmIgnoredWhenInvalidAndDegenerateInputs() {
        let whole = WaveformViewport()
        XCTAssertEqual(
            labels(WaveformRuler.ticks(duration: 2, bpm: 0, viewport: whole, width: 600)),
            labels(WaveformRuler.ticks(duration: 2, bpm: nil, viewport: whole, width: 600)))
        XCTAssertTrue(WaveformRuler.ticks(duration: 0, bpm: 120, viewport: whole, width: 600).isEmpty)
        XCTAssertTrue(WaveformRuler.ticks(duration: 2, bpm: nil, viewport: whole, width: 0).isEmpty)
    }
}

final class PreviewGainTests: XCTestCase {
    func testDbToLinear() {
        XCTAssertEqual(PreviewGain.linear(db: 0), 1, accuracy: 1e-12)
        XCTAssertEqual(PreviewGain.linear(db: -6), 0.5012, accuracy: 1e-3)
        XCTAssertEqual(PreviewGain.linear(db: 6), 1.9953, accuracy: 1e-3)
        XCTAssertEqual(PreviewGain.linear(db: -20), 0.1, accuracy: 1e-9)
    }

    func testClampAndRounding() {
        XCTAssertEqual(PreviewGain.clamped(-40), -24)
        XCTAssertEqual(PreviewGain.clamped(10), 6)
        XCTAssertEqual(PreviewGain.clamped(.nan), 0)
        XCTAssertEqual(PreviewGain.clamped(-3.04), -3.0)
        XCTAssertEqual(PreviewGain.linear(db: -100), PreviewGain.linear(db: -24), accuracy: 1e-12)
    }

    func testLabel() {
        XCTAssertEqual(PreviewGain.label(db: -3), "\u{2212}3.0 dB")
        XCTAssertEqual(PreviewGain.label(db: 1.5), "+1.5 dB")
        XCTAssertEqual(PreviewGain.label(db: 0), "0.0 dB")
    }

    func testKnobStepsInTenths() {
        XCTAssertEqual(Knob.value(start: 0, dy: -12, in: PreviewGain.rangeTenths, step: PreviewGain.stepTenths), 10)
        XCTAssertEqual(Knob.value(start: 0, dy: -12, in: PreviewGain.rangeTenths, step: PreviewGain.fineStepTenths), 2)
        XCTAssertEqual(Knob.value(start: 55, dy: -60, in: PreviewGain.rangeTenths, step: 5), 60)  // clamped at +6 dB
    }

    @MainActor
    func testPersistsAndReachesThePlayer() throws {
        let name = "gain-\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: name))
        defer { UserDefaults().removePersistentDomain(forName: name) }
        let player = FakePlayer()
        let preview = AudioPreview(player: player, defaults: defaults)
        XCTAssertEqual(preview.gainDb, 0)
        preview.setGain(db: -3.5)
        XCTAssertEqual(player.gains.last, -3.5)
        preview.setGain(db: 99)
        XCTAssertEqual(preview.gainDb, 6)

        let again = FakePlayer()
        preview.setGain(db: -7.5)
        let reopened = AudioPreview(player: again, defaults: defaults)
        XCTAssertEqual(reopened.gainDb, -7.5)
        XCTAssertEqual(again.gains.last, -7.5)  // applied to the engine at start
        reopened.resetGain()
        XCTAssertEqual(reopened.gainDb, 0)
        XCTAssertEqual(AudioPreview(player: FakePlayer(), defaults: defaults).gainDb, 0)
    }
}
