import AVFoundation
import Foundation

/// A fully decoded preview sound (Float32, deinterleaved). The buffer is
/// never mutated after creation, so sharing it across threads is safe.
final class PreviewClip: @unchecked Sendable {
    let buffer: AVAudioPCMBuffer
    init(buffer: AVAudioPCMBuffer) { self.buffer = buffer }

    /// Memory held by the samples (cache accounting).
    var byteCount: Int {
        Int(buffer.frameLength) * Int(buffer.format.channelCount) * MemoryLayout<Float>.size
    }
    var duration: TimeInterval {
        buffer.format.sampleRate > 0 ? Double(buffer.frameLength) / buffer.format.sampleRate : 0
    }
}

/// Source of decoded clips (the real decoder in the app, fakes in tests).
protocol PreviewDecoding: Sendable {
    /// Decodes the whole file off the caller's thread. Throws on failure.
    func decode(_ row: FileRow) async throws -> PreviewClip
}

enum PreviewDecodeError: Error, Equatable {
    case empty
}

/// Decodes with AVAudioFile and, for formats AVFoundation cannot open
/// (e.g. Ogg Vorbis), falls back to the core's `previewPcm` (ADR-032).
struct CoreDecoder: PreviewDecoding {
    let engine: SonarEngine

    func decode(_ row: FileRow) async throws -> PreviewClip {
        let engine = engine
        let id = row.fileId
        let url = URL(fileURLWithPath: row.path)
        return try await Task.detached(priority: .userInitiated) {
            try PreviewDecoder.decode(url: url, fallback: { try engine.previewPcm(fileId: id) })
        }.value
    }
}

/// Blocking decode helpers (call off the main actor).
enum PreviewDecoder {
    /// Longest decoded preview (same cap as the core), bounds memory.
    static let maxSeconds = 600.0

    static func decode(url: URL, fallback: () throws -> PreviewPcm) throws -> PreviewClip {
        if let clip = try? readWithAVFoundation(url) { return clip }
        return try clip(from: try fallback())
    }

    /// Reads up to `maxSeconds` through AVAudioFile (processing format is
    /// Float32 deinterleaved at the file's rate and channel count).
    static func readWithAVFoundation(_ url: URL) throws -> PreviewClip {
        let file = try AVAudioFile(forReading: url)
        let format = file.processingFormat
        // More than stereo: use the core's mono downmix instead (the
        // TimePitch/EQ chain is not guaranteed to accept surround formats,
        // and an AVFoundation exception cannot be caught in Swift).
        guard format.channelCount <= 2 else { throw PreviewDecodeError.empty }
        // A corrupt header can claim absurd rates/lengths: reject, never trap.
        guard (1_000...768_000).contains(format.sampleRate) else { throw PreviewDecodeError.empty }
        let frames = AVAudioFrameCount(
            clamping: min(file.length, Int64(format.sampleRate * maxSeconds)))
        guard frames > 0,
            let buffer = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: frames)
        else { throw PreviewDecodeError.empty }
        try file.read(into: buffer, frameCount: frames)
        guard buffer.frameLength > 0 else { throw PreviewDecodeError.empty }
        return PreviewClip(buffer: buffer)
    }

    /// Builds a clip from the core's interleaved PCM.
    static func clip(from pcm: PreviewPcm) throws -> PreviewClip {
        let channels = max(Int(pcm.channels), 1)
        let frames = pcm.samples.count / channels
        guard frames > 0, pcm.sampleRate > 0,
            let format = AVAudioFormat(
                commonFormat: .pcmFormatFloat32, sampleRate: Double(pcm.sampleRate),
                channels: AVAudioChannelCount(channels), interleaved: false),
            let buffer = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: AVAudioFrameCount(frames)),
            let out = buffer.floatChannelData
        else { throw PreviewDecodeError.empty }
        buffer.frameLength = AVAudioFrameCount(frames)
        for c in 0..<channels {
            for i in 0..<frames { out[c][i] = pcm.samples[i * channels + c] }
        }
        return PreviewClip(buffer: buffer)
    }
}

/// Small LRU of decoded clips with an entry and a byte cap. The newest entry
/// is always kept, even when it alone exceeds the byte cap.
@MainActor
final class PreviewCache {
    let maxEntries: Int
    let maxBytes: Int
    private var clips: [Int64: PreviewClip] = [:]
    private var order: [Int64] = []  // least recently used first
    private(set) var totalBytes = 0

    init(maxEntries: Int = 8, maxBytes: Int = 256 << 20) {
        self.maxEntries = maxEntries
        self.maxBytes = maxBytes
    }

    var count: Int { clips.count }
    var ids: [Int64] { order }
    func contains(_ id: Int64) -> Bool { clips[id] != nil }

    /// The clip, marking it most recently used.
    func get(_ id: Int64) -> PreviewClip? {
        guard let clip = clips[id] else { return nil }
        touch(id)
        return clip
    }

    func put(_ id: Int64, _ clip: PreviewClip) {
        if let old = clips[id] { totalBytes -= old.byteCount }
        clips[id] = clip
        totalBytes += clip.byteCount
        touch(id)
        while clips.count > 1, clips.count > maxEntries || totalBytes > maxBytes {
            let victim = order.removeFirst()
            if let gone = clips.removeValue(forKey: victim) { totalBytes -= gone.byteCount }
        }
    }

    private func touch(_ id: Int64) {
        order.removeAll { $0 == id }
        order.append(id)
    }
}
