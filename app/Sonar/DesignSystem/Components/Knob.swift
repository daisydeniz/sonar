import SwiftUI

/// Rotary knob for a stepped integer value (DESIGN §4.8), DAW-style:
/// vertical drag changes the value (up = more), double-click resets to
/// `defaultValue`, arrow keys / VoiceOver adjust by one step. No reset button.
struct Knob: View {
    @Binding var value: Int
    let range: ClosedRange<Int>
    let defaultValue: Int
    let accessibilityLabel: String

    /// Drag distance (pt) for one step.
    static let pointsPerStep: CGFloat = 6
    /// Sweep of the arc: 270°, gap at the bottom.
    private static let sweep = 0.75
    private static let startAngle = Angle.degrees(135)

    @State private var dragStartValue: Int?
    @FocusState private var focused: Bool

    init(value: Binding<Int>, in range: ClosedRange<Int>, defaultValue: Int = 0, accessibilityLabel: String) {
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

    var body: some View {
        let line = Theme.Size.knobStroke
        let current = fraction(value)
        let origin = fraction(min(max(defaultValue, range.lowerBound), range.upperBound))
        ZStack {
            Circle()
                .trim(from: 0, to: Self.sweep)
                .stroke(.quaternary, style: StrokeStyle(lineWidth: line, lineCap: .round))
                .rotationEffect(Self.startAngle)
                .padding(line / 2)
            Circle()
                .trim(from: min(origin, current) * Self.sweep, to: max(origin, current) * Self.sweep)
                .stroke(Theme.Color.accent, style: StrokeStyle(lineWidth: line, lineCap: .round))
                .rotationEffect(Self.startAngle)
                .padding(line / 2)
            Circle()
                .fill(Theme.Color.surfaceRaised)
                .overlay(Circle().strokeBorder(.tertiary, lineWidth: Theme.hairlineWidth * 2))
                .padding(line * 2)
            Capsule()
                .fill(Color.primary)
                .frame(width: line, height: Theme.Size.knob / 2 - line * 3)
                .offset(y: -(Theme.Size.knob / 4 - line * 0.5))
                .rotationEffect(.degrees(-135 + current * 270))
        }
        .frame(width: Theme.Size.knob, height: Theme.Size.knob)
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
