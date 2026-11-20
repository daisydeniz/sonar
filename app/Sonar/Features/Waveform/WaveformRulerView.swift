import AppKit

/// Draws the ruler ticks and labels (DESIGN §4.4) in `wave`: long ticks at
/// bars / beats / labelled steps, short subdivision ticks, labels in
/// monospaced `caption`. Never takes mouse events (the waveform view under
/// it handles click and drag).
final class WaveformRulerView: NSView {
    var ticks: [RulerTick] = [] {
        didSet { if ticks != oldValue { needsDisplay = true } }
    }

    override var isFlipped: Bool { true }
    override func hitTest(_ point: NSPoint) -> NSView? { nil }

    override func draw(_ dirtyRect: NSRect) {
        let wave = Theme.NS.wave
        let longTick = bounds.height * Self.longTickFraction
        let shortTick = bounds.height * Self.shortTickFraction
        let attributes: [NSAttributedString.Key: Any] = [
            .font: Theme.NSFonts.captionNumeric, .foregroundColor: wave,
        ]
        wave.withAlphaComponent(Self.tickOpacity).setFill()
        for tick in ticks {
            let x = (tick.fraction - viewportStart) / viewportSpan * bounds.width
            guard x >= -1, x <= bounds.width + 1 else { continue }
            let height = tick.major ? longTick : shortTick
            NSRect(x: x - Self.tickWidth / 2, y: bounds.height - height, width: Self.tickWidth, height: height).fill()
        }
        for tick in ticks {
            guard let label = tick.label else { continue }
            let x = (tick.fraction - viewportStart) / viewportSpan * bounds.width
            guard x >= -1, x <= bounds.width else { continue }
            (label as NSString).draw(
                at: NSPoint(x: x + Theme.Space.s4, y: Theme.Space.s2), withAttributes: attributes)
        }
        Theme.NS.waveCentre.setFill()
        NSRect(x: 0, y: bounds.height - Self.tickWidth, width: bounds.width, height: Self.tickWidth).fill()
    }

    /// Viewport of the ticks' fractions (set with them by `WaveformNSView`).
    var viewportStart = 0.0 { didSet { if viewportStart != oldValue { needsDisplay = true } } }
    var viewportSpan = 1.0 { didSet { if viewportSpan != oldValue { needsDisplay = true } } }

    private static let longTickFraction: CGFloat = 0.45
    private static let shortTickFraction: CGFloat = 0.22
    private static let tickWidth: CGFloat = 1
    private static let tickOpacity: CGFloat = 0.8
}
