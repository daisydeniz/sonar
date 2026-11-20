import SwiftUI

/// Rotary knob for a stepped integer value (DESIGN §4.8, ADR-043), DAW-style:
/// a coloured face in its parameter family colour with a dark pointer, a 270°
/// track and a value arc from the default to the value. Vertical drag changes
/// the value (up = more), double-click resets to `defaultValue`, arrow keys /
/// VoiceOver adjust by one step. No reset button.
struct Knob: View {
    /// Parameter family: sets the face colour (never the individual knob).
    enum Family {
        case pitch, gain, filter, time, neutral

        var face: Color {
            switch self {
            case .pitch: return Theme.Color.knobPitch
            case .gain: return Theme.Color.knobGain
            case .filter: return Theme.Color.knobFilter
            case .time: return Theme.Color.knobTime
            case .neutral: return Theme.Color.knobNeutral
            }
        }
    }

    /// 24 pt in bars, 44 pt in panels.
    enum Size {
        case small, large
        var diameter: CGFloat { self == .small ? Theme.Size.knobSmall : Theme.Size.knobLarge }
    }

    @Binding var value: Int
    let range: ClosedRange<Int>
    let defaultValue: Int
    let accessibilityLabel: String
    var family: Family = .pitch
    var size: Size = .small

    /// Drag distance (pt) for one step.
    static let pointsPerStep: CGFloat = 6
    /// Sweep of the arc: 270°, gap at the bottom.
    private static let sweep = 0.75
    private static let startAngle = Angle.degrees(135)

    @State private var dragStartValue: Int?
    @FocusState private var focused: Bool
    @Environment(\.colorScheme) private var scheme

    init(
        value: Binding<Int>, in range: ClosedRange<Int>, defaultValue: Int = 0, accessibilityLabel: String,
        family: Family = .pitch, size: Size = .small
    ) {
        self.family = family
        self.size = size
        _value = value
        self.range = range
        self.defaultValue = defaultValue
        self.accessibilityLabel = accessibilityLabel
    }

    /// Value after a vertical drag of `dy` points (negative = up) from `start`.
    static func value(start: Int, dy: CGFloat, in range: ClosedRange<Int>) -> Int {
        let steps = Int((-dy / pointsPerStep).rounded(.towardZero))
        return min(max(start + steps, range.lowerBound), range.upperBound)
    }

    private func fraction(_ v: Int) -> Double {
        Double(v - range.lowerBound) / Double(max(range.upperBound - range.lowerBound, 1))
    }

    private func set(_ new: Int) {
        let clamped = min(max(new, range.lowerBound), range.upperBound)
        if clamped != value { value = clamped }
    }

    /// Pointer: black at 55 %, white on the dark neutral face.
    private var pointerColor: Color {
        family == .neutral && scheme == .dark ? Color.white : Color.black.opacity(Theme.knobPointerOpacity)
    }

    /// Value arc: the face colour (the neutral face is too close to the track).
    private var arcColor: Color {
        family == .neutral ? Color.primary.opacity(Theme.knobNeutralArcOpacity) : family.face
    }

    var body: some View {
        let d = size.diameter
        let arc = Theme.Size.knobArc
        let outer = d + arc * 4
        let ring = d + arc * 2
        let current = fraction(value)
        let origin = fraction(min(max(defaultValue, range.lowerBound), range.upperBound))
        let angle = Angle.degrees(-135 + current * 270)
        ZStack {
            Circle()
                .trim(from: 0, to: Self.sweep)
                .stroke(Color.primary.opacity(Theme.knobTrackOpacity), style: StrokeStyle(lineWidth: arc, lineCap: .round))
                .rotationEffect(Self.startAngle)
                .frame(width: ring, height: ring)
            Circle()
                .trim(from: min(origin, current) * Self.sweep, to: max(origin, current) * Self.sweep)
                .stroke(arcColor, style: StrokeStyle(lineWidth: arc, lineCap: .round))
                .rotationEffect(Self.startAngle)
                .frame(width: ring, height: ring)
            Circle()
                .fill(family.face)
                .overlay(Circle().strokeBorder(Color.black.opacity(Theme.knobRimOpacity), lineWidth: Theme.hairlineWidth * 2))
                .overlay {
                    Circle()
                        .trim(from: 0.64, to: 0.86)
                        .stroke(Color.white.opacity(Theme.knobHighlightOpacity), style: StrokeStyle(lineWidth: Theme.hairlineWidth * 2, lineCap: .round))
                        .padding(Theme.hairlineWidth * 4)
                }
                .frame(width: d, height: d)
            Capsule()
                .fill(pointerColor)
                .frame(width: Theme.hairlineWidth * 3, height: d / 2 * 0.57)
                .offset(y: -(d / 2 * 0.535))
                .rotationEffect(angle)
        }
        .frame(width: outer, height: outer)
        .contentShape(Circle())
        .gesture(
            DragGesture(minimumDistance: 1)
                .onChanged { drag in
                    let start = dragStartValue ?? value
                    if dragStartValue == nil { dragStartValue = start }
                    set(Self.value(start: start, dy: drag.translation.height, in: range))
                }
                .onEnded { _ in dragStartValue = nil }
        )
        .onTapGesture(count: 2) { set(defaultValue) }
        .focusable()
        .focused($focused)
        .onKeyPress(.upArrow) { set(value + 1); return .handled }
        .onKeyPress(.rightArrow) { set(value + 1); return .handled }
        .onKeyPress(.downArrow) { set(value - 1); return .handled }
        .onKeyPress(.leftArrow) { set(value - 1); return .handled }
        .help("Drag up/down to change, double-click to reset")
        .accessibilityElement()
        .accessibilityLabel(accessibilityLabel)
        .accessibilityValue("\(value)")
        .accessibilityAdjustableAction { direction in
            switch direction {
            case .increment: set(value + 1)
            case .decrement: set(value - 1)
            @unknown default: break
            }
        }
        .accessibilityAction(named: "Reset") { set(defaultValue) }
    }
}
