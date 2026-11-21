import AppKit
import QuartzCore

/// Layer-backed waveform (DESIGN §4.4): an optional ruler on top, one lane
/// per channel (min/max columns for the visible range, centre line, lane
/// dividers) and a playhead with a triangle head in the ruler. Re-bucketing
/// for the viewport is display math only (`WaveformMath`); peaks come from
/// the core.
@MainActor
final class WaveformNSView: NSView {
    /// One list per channel (one lane each); empty = flat centre line.
    var peaks: [[WaveformPeak]] = [] { didSet { if peaks != oldValue { relayout() } } }
    var viewport = WaveformViewport() { didSet { if viewport != oldValue { relayout() } } }
    /// Ruler height (0 = no ruler, as in the compare target well).
    var rulerHeight: CGFloat = 0 { didSet { if rulerHeight != oldValue { relayout() } } }
    /// Tempo for the bars.beats ruler; nil = seconds.
    var bpm: Double? { didSet { if bpm != oldValue { relayout() } } }
    /// Amplitude zoom of the lanes (display only).
    var amplitudeZoom = 1.0 { didSet { if amplitudeZoom != oldValue { relayout() } } }
    /// Playhead source (fraction 0...1 or nil); polled only while `isPlaying`.
    var playbackFraction: (() -> Double?)?
    /// Click: seek to this sound fraction.
    var onSeek: ((Double) -> Void)?
    /// Selected region (drawn in time coordinates, so it follows zoom/pan).
    var region: WaveformRegion? { didSet { if region != oldValue { placeRegion() } } }
    /// Duration (s) of the sound; regions are fractions of it.
    var duration: Double? { didSet { if duration != oldValue { relayout() } } }
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

    /// Layers of one channel lane. Played = the same shape as unplayed (at
    /// 40 %), masked to the left of the playhead.
    private struct Lane {
        let container = CALayer()
        let centre = CALayer()
        let unplayed = CAShapeLayer()
        let played = CAShapeLayer()
        let mask = CALayer()
    }

    private var lanes: [Lane] = []
    private var dividers: [CALayer] = []
    private let ruler = WaveformRulerView()
    private let playhead = CALayer()
    private let playheadHead = CAShapeLayer()
    private let regionTint = CALayer()
    private let regionEdges = [CALayer(), CALayer()]
    private let regionHandles = [CALayer(), CALayer()]
    private var drag: Drag?
    /// The current drag has produced a region (SwiftUI applies it a beat later).
    private var displayLink: CADisplayLink?
    private var playheadFraction: Double?

    private var hasPeaks: Bool { peaks.contains { !$0.isEmpty } }
    /// Height of the lanes' area (below the ruler).
    private var lanesHeight: CGFloat { max(bounds.height - rulerHeight, 0) }

    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
        addSubview(ruler)
        for l in [regionTint] + regionEdges + regionHandles {
            l.isHidden = true
            layer?.addSublayer(l)
        }
        for l in regionHandles { l.cornerRadius = Theme.Radius.small }
        playhead.isHidden = true
        playheadHead.isHidden = true
        layer?.addSublayer(playhead)
        layer?.addSublayer(playheadHead)
        setAccessibilityElement(true)
        setAccessibilityRole(.image)
        setAccessibilityLabel("Waveform")
        ensureLanes(1)
        updateColors()
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    override var isFlipped: Bool { true }
    override var acceptsFirstResponder: Bool { false }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        updateColors()
        ruler.needsDisplay = true
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
            for lane in lanes {
                lane.centre.backgroundColor = Theme.NS.waveCentre.cgColor
                lane.played.strokeColor = Theme.NS.wave.cgColor
                lane.unplayed.strokeColor = Theme.NS.wave.withAlphaComponent(Theme.waveformUnplayedOpacity).cgColor
            }
            for d in dividers { d.backgroundColor = Theme.NS.waveGrid.cgColor }
            playhead.backgroundColor = NSColor.labelColor.cgColor
            playheadHead.fillColor = NSColor.labelColor.cgColor
            regionTint.backgroundColor = Theme.NS.selection.cgColor
            for l in regionEdges + regionHandles { l.backgroundColor = Theme.NS.accent.cgColor }
        }
    }

    /// Makes exactly `count` lanes (and `count - 1` dividers).
    private func ensureLanes(_ count: Int) {
        guard lanes.count != count else { return }
        for lane in lanes { lane.container.removeFromSuperlayer() }
        for d in dividers { d.removeFromSuperlayer() }
        lanes = (0..<count).map { _ in
            let lane = Lane()
            for shape in [lane.unplayed, lane.played] {
                shape.fillColor = nil
                shape.lineCap = .butt
            }
            lane.mask.backgroundColor = Theme.NS.wave.cgColor  // mask alpha only
            lane.played.mask = lane.mask
            for l in [lane.centre, lane.unplayed, lane.played] { lane.container.addSublayer(l) }
            return lane
        }
        dividers = (1..<max(count, 1)).map { _ in CALayer() }
        // Lanes sit below the region / playhead layers.
        for lane in lanes { layer?.insertSublayer(lane.container, at: 0) }
        for d in dividers { layer?.insertSublayer(d, at: 0) }
        updateColors()
    }

    private func relayout() {
        let size = bounds.size
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        defer { CATransaction.commit() }
        ensureLanes(max(1, peaks.count))
        let n = CGFloat(lanes.count)
        let divider = Theme.Size.laneDivider
        let laneHeight = max((lanesHeight - (n - 1) * divider) / n, 0)
        let scale = window?.backingScaleFactor ?? 2
        // One column per device pixel pair keeps the path small and crisp.
        let count = max(1, Int(size.width * scale / 2))
        let laneSize = CGSize(width: size.width, height: laneHeight)
        for (i, lane) in lanes.enumerated() {
            let y = rulerHeight + CGFloat(i) * (laneHeight + divider)
            lane.container.frame = CGRect(x: 0, y: y, width: size.width, height: laneHeight)
            let local = CGRect(origin: .zero, size: laneSize)
            lane.centre.frame = CGRect(x: 0, y: laneHeight / 2 - 0.5, width: size.width, height: 1)
            lane.unplayed.frame = local
            lane.played.frame = local
            var columns = i < peaks.count
                ? WaveformMath.rebucket(peaks[i], from: viewport.start, to: viewport.end, into: count) : []
            if amplitudeZoom != 1 {
                let z = Float(amplitudeZoom)
                columns = columns.map { WaveformPeak(min: $0.min * z, max: $0.max * z) }
            }
            if columns.isEmpty {
                lane.unplayed.path = nil
                lane.played.path = nil
            } else {
                let path = WaveformMath.path(columns: columns, size: laneSize)
                lane.unplayed.path = path
                lane.played.path = path
                lane.unplayed.lineWidth = size.width / CGFloat(columns.count)
                lane.played.lineWidth = lane.unplayed.lineWidth
            }
            if i > 0 {
                dividers[i - 1].frame = CGRect(x: 0, y: y - divider, width: size.width, height: divider)
            }
        }
        ruler.frame = CGRect(x: 0, y: 0, width: size.width, height: rulerHeight)
        ruler.isHidden = rulerHeight == 0
        ruler.viewportStart = viewport.start
        ruler.viewportSpan = viewport.span
        if let duration, hasPeaks, rulerHeight > 0 {
            ruler.ticks = WaveformRuler.ticks(
                duration: duration, bpm: bpm, viewport: viewport, width: Double(size.width))
        } else {
            ruler.ticks = []
        }
        placePlayhead()
        placeRegion()
    }

    /// Region tint, 1 pt accent edges and drag handles at the viewport's x.
    private func placeRegion() {
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        defer { CATransaction.commit() }
        guard let region, hasPeaks else {
            for l in [regionTint] + regionEdges + regionHandles { l.isHidden = true }
            window?.invalidateCursorRects(for: self)
            return
        }
        let top = rulerHeight
        let height = lanesHeight
        let xs = [region.start, region.end].map { viewport.x(forFraction: $0, width: bounds.width) }
        let left = min(max(xs[0], 0), bounds.width)
        let right = min(max(xs[1], 0), bounds.width)
        regionTint.isHidden = right <= left
        regionTint.frame = CGRect(x: left, y: top, width: max(right - left, 0), height: height)
        let edgeWidth = Theme.Size.regionEdge
        for (i, x) in xs.enumerated() {
            let visible = x >= 0 && x <= bounds.width
            regionEdges[i].isHidden = !visible
            regionHandles[i].isHidden = !visible
            regionEdges[i].frame = CGRect(x: x - edgeWidth / 2, y: top, width: edgeWidth, height: height)
            let hw = Theme.Size.regionHandleWidth, hh = Theme.Size.regionHandleHeight
            regionHandles[i].frame = CGRect(x: x - hw / 2, y: top + (height - hh) / 2, width: hw, height: hh)
        }
        window?.invalidateCursorRects(for: self)
    }

    private func placePlayhead() {
        guard let f = playheadFraction, hasPeaks else {
            playhead.isHidden = true
            playheadHead.isHidden = true
            for lane in lanes { lane.mask.frame = .zero }
            return
        }
        let x = viewport.x(forFraction: f, width: bounds.width)
        let visible = x >= 0 && x <= bounds.width
        playhead.isHidden = !visible
        playhead.frame = CGRect(x: x - 0.5, y: 0, width: 1, height: bounds.height)
        let head = Theme.Size.playheadHead
        playheadHead.isHidden = !visible || rulerHeight == 0
        playheadHead.frame = CGRect(x: x - head / 2, y: 0, width: head, height: head)
        playheadHead.path = {
            let p = CGMutablePath()
            p.move(to: .zero)
            p.addLine(to: CGPoint(x: head, y: 0))
            p.addLine(to: CGPoint(x: head / 2, y: head))
            p.closeSubpath()
            return p
        }()
        for lane in lanes {
            lane.mask.frame = CGRect(
                x: 0, y: 0, width: min(max(x, 0), bounds.width), height: lane.container.bounds.height)
        }
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
        case pending(RegionGesture)
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
        guard hasPeaks else { return }
        let x = convert(event.locationInWindow, from: nil).x
        if let edge = edge(atX: x) {
            drag = .edge(edge)
        } else {
            drag = .pending(RegionGesture(
                anchor: viewport.fraction(atX: x, width: bounds.width), startX: x, threshold: Self.dragThreshold))
        }
    }

    override func mouseDragged(with event: NSEvent) {
        let x = convert(event.locationInWindow, from: nil).x
        let fraction = viewport.fraction(atX: x, width: bounds.width)
        switch drag {
        case .edge(let edge):
            if let region, let duration { onRegion?(region.resized(edge, to: fraction, duration: duration)) }
        case .pending(var gesture):
            let result = gesture.dragged(toX: x, fraction: fraction, duration: duration)
            drag = .pending(gesture)
            if case .region(let made) = result { onRegion?(made) }
        case nil:
            break
        }
    }

    override func mouseUp(with event: NSEvent) {
        defer { drag = nil }
        guard case .pending(let gesture) = drag else { return }
        // A click (a press that never passed the drag threshold): seek. A click outside
        // the region clears it; inside, the region keeps playing from there.
        switch gesture.released(region: region) {
        case .keep: return
        case .seek(let fraction, let clear):
            if clear { onRegion?(nil) }
            onSeek?(fraction)
        }
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
        guard hasPeaks else { return nil }
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
