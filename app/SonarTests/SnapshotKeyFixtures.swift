import Foundation
@testable import Sonar

extension SnapshotFixtures {
    /// Rows 0...7 cover every key display case (ADR-041): high / medium /
    /// low / confidence 0 / en dash / name-audio disagreement / detuned
    /// one-shots (E2 +32 ct, ≈ past 25 ct); the rest are the usual fixtures.
    static func keyRows() -> [FileRow] {
        var rows = rows()
        func set(
            _ i: Int, name: String, kind: String, category: String, key: String?, conf: Float?,
            source: String?, keyName: String? = nil, keyAudio: String? = nil, note: String? = nil, cents: Float? = nil
        ) {
            var r = rows[i]
            r.name = name
            r.path = "/Samples/KSHMR Vol 1/Keys/\(name).wav"
            r.kind = kind
            r.category = category
            r.bpm = kind == "loop" ? 120 : nil
            r.key = key
            r.keyConf = conf
            r.keySource = source
            r.keyName = keyName
            r.keyAudio = keyAudio
            r.noteName = note
            r.noteCents = cents
            r.hidden = false
            rows[i] = r
        }
        set(0, name: "Key_High_Am_Pad", kind: "loop", category: "pad", key: "Am", conf: 0.95, source: "both")
        set(1, name: "Key_Medium_Pluck", kind: "loop", category: "pluck", key: "F#m", conf: 0.75, source: "detected")
        set(2, name: "Key_Low_Hat_Loop", kind: "loop", category: "hat", key: "C", conf: 0.3, source: "detected")
        set(3, name: "Key_Zero_Noise", kind: "one_shot", category: "fx", key: "G", conf: 0, source: "detected")
        set(4, name: "Key_Silence", kind: "one_shot", category: "fx", key: nil, conf: nil, source: nil)
        set(
            5, name: "Key_Disagree_Am_Bass", kind: "loop", category: "bass", key: "Cm", conf: 0.82,
            source: "detected", keyName: "Am")
        set(
            6, name: "Key_808_Detuned", kind: "one_shot", category: "bass", key: "E", conf: 0.93,
            source: "detected", note: "E2", cents: 18)
        set(
            7, name: "Key_808_Quarter_Tone", kind: "one_shot", category: "bass", key: "A#", conf: 0.64,
            source: "detected", note: "A#1", cents: -41)
        // The name outweighed a disagreeing audio key (source Name).
        set(
            8, name: "Key_NameWins_Dm_Pluck", kind: "loop", category: "pluck", key: "Dm", conf: 0.7,
            source: "filename", keyAudio: "F#m")
        return rows
    }
}
