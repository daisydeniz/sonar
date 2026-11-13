import AppKit
import QuartzCore

/// Layer-backed waveform: a `CAShapeLayer` of min/max columns for the visible
/// range plus a playhead line layer. Re-bucketing for the viewport is
/// display math only (`WaveformMath`); peaks come from the core.
@MainActor
final class WaveformNSView: NSView {
    var peaks: [WaveformPeak] = [] { didSet { if peaks != oldValue { relayout() } } }
    var viewport = WaveformViewport() { didSet { if viewport != oldValue { relayout() } } }
    /// Playhead source (fraction 0...1 or nil); polled only while `isPlaying`.
    var playbackFraction: (() -> Double?)?
    /// Click: seek to this sound fraction.
    var onSeek: ((Double) -> Void)?
    /// Zoom by factor around an anchor (0...1 of the width).
    var onZoom: ((Double, Double) -> Void)?
    /// Horizontal scroll by points.
    var onPan: ((Double) -> Void)?

    var isPlaying = false {
        didSet {
            guard isPlaying != oldValue else { return }
            isPlaying ? startDisplayLink() : stopDisplayLink()
        }
    }

    private let waveLayer = CAShapeLayer()
    private let playhead = CALayer()
    private var displayLink: CADisplayLink?
    private var playheadFraction: Double?

    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
        layer?.addSublayer(waveLayer)
        waveLayer.fillColor = nil
        waveLayer.lineCap = .butt
        playhead.isHidden = true
        layer?.addSublayer(playhead)
        setAccessibilityElement(true)
        setAccessibilityRole(.image)
        setAccessibilityLabel("Waveform")
        updateColors()
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    override var isFlipped: Bool { true }
    override var acceptsFirstResponder: Bool { false }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        updateColors()
    }

    override func layout() {
        super.layout()
        relayout()
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        if window == nil { stopDisplayLink() } else if isPlaying { startDisplayLink() }
    }

    // MARK: Drawing

    private func updateColors() {
        effectiveAppearance.performAsCurrentDrawingAppearance {
            waveLayer.strokeColor = NSColor.controlAccentColor.cgColor
            playhead.backgroundColor = NSColor.labelColor.cgColor
        }
    }

    private func relayout() {
        let size = bounds.size
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        defer { CATransaction.commit() }
        waveLayer.frame = bounds
        let scale = window?.backingScaleFactor ?? 2
        // One column per device pixel pair keeps the path small and crisp.
        let count = max(1, Int(size.width * scale / 2))
        let columns = WaveformMath.rebucket(peaks, from: viewport.start, to: viewport.end, into: count)
        waveLayer.lineWidth = columns.isEmpty ? 1 : size.width / CGFloat(columns.count)
        waveLayer.path = WaveformMath.path(columns: columns, size: size)
        placePlayhead()
    }

    private func placePlayhead() {
        guard let f = playheadFraction, !peaks.isEmpty else {
            playhead.isHidden = true
            return
        }
        let x = viewport.x(forFraction: f, width: bounds.width)
        playhead.isHidden = x < 0 || x > bounds.width
        playhead.frame = CGRect(x: x - 0.5, y: 0, width: 1, height: bounds.height)
    }

    // MARK: Playhead

    private func startDisplayLink() {
        guard window != nil, displayLink == nil else { return }
        let link = displayLink(target: self, selector: #selector(tick))
        link.add(to: .main, forMode: .common)
        displayLink = link
    }

    private func stopDisplayLink() {
        displayLink?.invalidate()
        displayLink = nil
        playheadFraction = nil
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        placePlayhead()
        CATransaction.commit()
    }

    @objc private func tick() {
        playheadFraction = playbackFraction?()
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        placePlayhead()
        CATransaction.commit()
    }

    // MARK: Input

    override func mouseDown(with event: NSEvent) {
        guard !peaks.isEmpty else { return }
        let x = convert(event.locationInWindow, from: nil).x
        onSeek?(viewport.fraction(atX: x, width: bounds.width))
    }

    override func magnify(with event: NSEvent) {
        let anchor = bounds.width > 0 ? convert(event.locationInWindow, from: nil).x / bounds.width : 0.5
        onZoom?(1 + Double(event.magnification), Double(anchor))
    }

    override func scrollWheel(with event: NSEvent) {
        let horizontal = abs(event.scrollingDeltaX) >= abs(event.scrollingDeltaY)
        guard event.modifierFlags.contains(.command) || (viewport.isZoomed && horizontal) else {
            // Let the inspector scroll vertically (Shift-wheel pans a mouse).
            super.scrollWheel(with: event)
            return
        }
        if event.modifierFlags.contains(.command) {
            let anchor = bounds.width > 0 ? convert(event.locationInWindow, from: nil).x / bounds.width : 0.5
            onZoom?(exp(Double(event.scrollingDeltaY) * 0.02), Double(anchor))
            return
        }
        onPan?(-Double(event.scrollingDeltaX))
    }
}
