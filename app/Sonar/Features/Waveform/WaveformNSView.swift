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
    /// Selected region (drawn in time coordinates, so it follows zoom/pan).
    var region: WaveformRegion? { didSet { if region != oldValue { placeRegion() } } }
    /// Duration (s) of the sound; regions need it for their 50 ms minimum.
    var duration: Double?
    /// A drag created, resized or (nil) cleared the region.
    var onRegion: ((WaveformRegion?) -> Void)?
    /// Context menu entries (nil = none).
    var onPlaySelection: (() -> Void)?
    var onFindSimilarToSelection: (() -> Void)?
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

    /// Unplayed part (accent at 45 %), the whole shape.
    private let unplayedLayer = CAShapeLayer()
    /// Played part (full accent): same shape, masked to the left of the playhead.
    private let waveLayer = CAShapeLayer()
    private let playedMask = CALayer()
    private let playhead = CALayer()
    private let regionTint = CALayer()
    private let regionEdges = [CALayer(), CALayer()]
    private let regionHandles = [CALayer(), CALayer()]
    private var drag: Drag?
    /// The current drag has produced a region (SwiftUI applies it a beat later).
    private var dragMadeRegion = false
    private var displayLink: CADisplayLink?
    private var playheadFraction: Double?

    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
        layer?.addSublayer(unplayedLayer)
        layer?.addSublayer(waveLayer)
        for shape in [unplayedLayer, waveLayer] {
            shape.fillColor = nil
            shape.lineCap = .butt
        }
        playedMask.backgroundColor = Theme.NS.accent.cgColor  // mask alpha only
        waveLayer.mask = playedMask
        for l in [regionTint] + regionEdges + regionHandles {
            l.isHidden = true
            layer?.addSublayer(l)
        }
        for l in regionHandles { l.cornerRadius = Theme.Radius.small }
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
            waveLayer.strokeColor = Theme.NS.accent.cgColor
            unplayedLayer.strokeColor = Theme.NS.accent.withAlphaComponent(Theme.waveformUnplayedOpacity).cgColor
            playhead.backgroundColor = NSColor.labelColor.cgColor
            regionTint.backgroundColor = Theme.NS.selection.cgColor
            for l in regionEdges + regionHandles { l.backgroundColor = Theme.NS.accent.cgColor }
        }
    }

    private func relayout() {
        let size = bounds.size
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        defer { CATransaction.commit() }
        unplayedLayer.frame = bounds
        waveLayer.frame = bounds
        let scale = window?.backingScaleFactor ?? 2
        // One column per device pixel pair keeps the path small and crisp.
        let count = max(1, Int(size.width * scale / 2))
        let columns = WaveformMath.rebucket(peaks, from: viewport.start, to: viewport.end, into: count)
        let path: CGPath
        if columns.isEmpty {
            // No peaks (none selected / loading): a flat centre line.
            let line = CGMutablePath()
            line.move(to: CGPoint(x: 0, y: size.height / 2))
            line.addLine(to: CGPoint(x: size.width, y: size.height / 2))
            path = line
            unplayedLayer.lineWidth = 1
        } else {
            path = WaveformMath.path(columns: columns, size: size)
            unplayedLayer.lineWidth = size.width / CGFloat(columns.count)
        }
        waveLayer.lineWidth = unplayedLayer.lineWidth
        unplayedLayer.path = path
        waveLayer.path = columns.isEmpty ? nil : path
        placePlayhead()
        placeRegion()
    }

    /// Region tint, 1 pt accent edges and drag handles at the viewport's x.
    private func placeRegion() {
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        defer { CATransaction.commit() }
        guard let region, !peaks.isEmpty else {
            for l in [regionTint] + regionEdges + regionHandles { l.isHidden = true }
            window?.invalidateCursorRects(for: self)
            return
        }
        let xs = [region.start, region.end].map { viewport.x(forFraction: $0, width: bounds.width) }
        let left = min(max(xs[0], 0), bounds.width)
        let right = min(max(xs[1], 0), bounds.width)
        regionTint.isHidden = right <= left
        regionTint.frame = CGRect(x: left, y: 0, width: max(right - left, 0), height: bounds.height)
        let edgeWidth = Theme.Size.regionEdge
        for (i, x) in xs.enumerated() {
            let visible = x >= 0 && x <= bounds.width
            regionEdges[i].isHidden = !visible
            regionHandles[i].isHidden = !visible
            regionEdges[i].frame = CGRect(x: x - edgeWidth / 2, y: 0, width: edgeWidth, height: bounds.height)
            let hw = Theme.Size.regionHandleWidth, hh = Theme.Size.regionHandleHeight
            regionHandles[i].frame = CGRect(x: x - hw / 2, y: (bounds.height - hh) / 2, width: hw, height: hh)
        }
        window?.invalidateCursorRects(for: self)
    }

    private func placePlayhead() {
        guard let f = playheadFraction, !peaks.isEmpty else {
            playhead.isHidden = true
            playedMask.frame = .zero
            return
        }
        let x = viewport.x(forFraction: f, width: bounds.width)
        playhead.isHidden = x < 0 || x > bounds.width
        playhead.frame = CGRect(x: x - 0.5, y: 0, width: 1, height: bounds.height)
        playedMask.frame = CGRect(x: 0, y: 0, width: min(max(x, 0), bounds.width), height: bounds.height)
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

    /// Mouse gesture in progress.
    private enum Drag {
        /// Might become a new region; a click when it stays short.
        case pending(anchor: Double, startX: CGFloat, moved: Bool)
        case edge(RegionEdge)
    }

    /// Points a press may move before it counts as a drag.
    private static let dragThreshold = Theme.Space.s4

    private func edge(atX x: CGFloat) -> RegionEdge? {
        guard let region else { return nil }
        let slop = Theme.Space.s6
        let s = viewport.x(forFraction: region.start, width: bounds.width)
        let e = viewport.x(forFraction: region.end, width: bounds.width)
        let ds = abs(x - s), de = abs(x - e)
        guard min(ds, de) <= slop else { return nil }
        return ds <= de ? .start : .end
    }

    override func mouseDown(with event: NSEvent) {
        guard !peaks.isEmpty else { return }
        let x = convert(event.locationInWindow, from: nil).x
        if let edge = edge(atX: x) {
            drag = .edge(edge)
        } else {
            drag = .pending(anchor: viewport.fraction(atX: x, width: bounds.width), startX: x, moved: false)
        }
        dragMadeRegion = false
    }

    override func mouseDragged(with event: NSEvent) {
        let x = convert(event.locationInWindow, from: nil).x
        let fraction = viewport.fraction(atX: x, width: bounds.width)
        switch drag {
        case .edge(let edge):
            if let region, let duration { onRegion?(region.resized(edge, to: fraction, duration: duration)) }
        case .pending(let anchor, let startX, let moved):
            guard moved || abs(x - startX) > Self.dragThreshold else { return }
            drag = .pending(anchor: anchor, startX: startX, moved: true)
            // Shorter than 50 ms: no region yet (a release there is a click).
            let made = duration.flatMap { WaveformRegion(from: anchor, to: fraction, duration: $0) }
            dragMadeRegion = made != nil
            onRegion?(made)
        case nil:
            break
        }
    }

    override func mouseUp(with event: NSEvent) {
        defer { drag = nil }
        guard case .pending(let anchor, _, let moved) = drag else { return }
        // A click (or a drag too short to be a region): seek. A click outside
        // the region clears it; inside, the region keeps playing from there.
        if moved && dragMadeRegion { return }
        if let region, !region.contains(anchor) { onRegion?(nil) }
        onSeek?(anchor)
    }

    override func resetCursorRects() {
        super.resetCursorRects()
        guard let region else { return }
        let slop = Theme.Space.s6
        for f in [region.start, region.end] {
            let x = viewport.x(forFraction: f, width: bounds.width)
            addCursorRect(CGRect(x: x - slop, y: 0, width: slop * 2, height: bounds.height), cursor: .resizeLeftRight)
        }
    }

    // MARK: Context menu

    override func menu(for event: NSEvent) -> NSMenu? {
        guard !peaks.isEmpty else { return nil }
        let menu = NSMenu()
        let hasRegion = region != nil
        let play = ActionMenuItem(title: "Play Selection", action: onPlaySelection)
        let similar = ActionMenuItem(title: "Find Similar to Selection", action: onFindSimilarToSelection)
        let clear = ActionMenuItem(title: "Clear Selection") { [weak self] in self?.onRegion?(nil) }
        for item in [play, similar, clear] {
            item.isEnabled = hasRegion
            menu.addItem(item)
        }
        return menu
    }

    override func magnify(with event: NSEvent) {
        let anchor = bounds.width > 0 ? convert(event.locationInWindow, from: nil).x / bounds.width : 0.5
        onZoom?(1 + Double(event.magnification), Double(anchor))
    }

    override func scrollWheel(with event: NSEvent) {
        let horizontal = abs(event.scrollingDeltaX) >= abs(event.scrollingDeltaY)
        guard event.modifierFlags.contains(.command) || (viewport.isZoomed && horizontal) else {
            // Let the content column scroll vertically (Shift-wheel pans a mouse).
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

/// Menu item that runs a closure.
private final class ActionMenuItem: NSMenuItem {
    private let handler: (() -> Void)?

    init(title: String, action handler: (() -> Void)?) {
        self.handler = handler
        super.init(title: title, action: #selector(run), keyEquivalent: "")
        target = self
    }

    required init(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    @objc private func run() { handler?() }
}
