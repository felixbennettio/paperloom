import AVFoundation
import CoreKit
import SwiftUI

/// Records the microphone into an AAC file, as OneNote records audio into a
/// page; the recording goes into the vault beside the note.
@MainActor
@Observable
final class AudioRecorder {
    private(set) var isRecording = false
    @ObservationIgnored private var recorder: AVAudioRecorder?
    @ObservationIgnored private var file: URL?

    /// Starts recording; why it cannot, when it cannot.
    func start() async -> String? {
        #if os(iOS)
        do {
            let session = AVAudioSession.sharedInstance()
            try session.setCategory(.playAndRecord, mode: .default, options: [.defaultToSpeaker])
            try session.setActive(true)
        } catch {
            return error.localizedDescription
        }
        let allowed = await AVAudioApplication.requestRecordPermission()
        #else
        let allowed = await AVCaptureDevice.requestAccess(for: .audio)
        #endif
        guard allowed else { return String(localized: "Allow Paperloom to use the microphone in Settings.") }
        let file = FileManager.default.temporaryDirectory.appending(path: "\(UUID().uuidString).m4a")
        let settings: [String: Any] = [
            AVFormatIDKey: kAudioFormatMPEG4AAC,
            AVSampleRateKey: 44_100,
            AVNumberOfChannelsKey: 1,
            AVEncoderAudioQualityKey: AVAudioQuality.high.rawValue,
        ]
        do {
            let recorder = try AVAudioRecorder(url: file, settings: settings)
            guard recorder.record() else { return String(localized: "Allow Paperloom to use the microphone in Settings.") }
            self.recorder = recorder
            self.file = file
            isRecording = true
            return nil
        } catch {
            return error.localizedDescription
        }
    }

    /// Stops; what was recorded.
    func stop() -> Data? {
        recorder?.stop()
        recorder = nil
        isRecording = false
        guard let file else { return nil }
        self.file = nil
        let data = try? Data(contentsOf: file)
        try? FileManager.default.removeItem(at: file)
        return data
    }
}

/// Plays a recording embedded in a note, with a small bar to pause or stop
/// it.
@MainActor
@Observable
final class AudioPlayer: NSObject, AVAudioPlayerDelegate {
    /// What is playing, if anything.
    private(set) var title: String?
    private(set) var isPlaying = false
    @ObservationIgnored private var player: AVAudioPlayer?

    func play(_ url: URL) {
        #if os(iOS)
        try? AVAudioSession.sharedInstance().setCategory(.playback)
        try? AVAudioSession.sharedInstance().setActive(true)
        #endif
        do {
            let player = try AVAudioPlayer(contentsOf: url)
            player.delegate = self
            player.play()
            self.player = player
            title = url.deletingPathExtension().lastPathComponent
            isPlaying = true
        } catch {
            stop()
        }
    }

    func toggle() {
        guard let player else { return }
        if player.isPlaying {
            player.pause()
        } else {
            player.play()
        }
        isPlaying = player.isPlaying
    }

    func stop() {
        player?.stop()
        player = nil
        title = nil
        isPlaying = false
    }

    nonisolated func audioPlayerDidFinishPlaying(_ player: AVAudioPlayer, successfully flag: Bool) {
        Task { @MainActor in self.isPlaying = false }
    }
}

/// The bar a playing recording shows over the note.
struct AudioPlayerBar: View {
    let player: AudioPlayer

    var body: some View {
        if let title = player.title {
            HStack(spacing: 12) {
                Image(systemName: "waveform")
                    .foregroundStyle(.secondary)
                Text(verbatim: title)
                    .lineLimit(1)
                Button(
                    player.isPlaying ? LocalizedStringKey("Pause") : LocalizedStringKey("Play"),
                    systemImage: player.isPlaying ? "pause.fill" : "play.fill"
                ) { player.toggle() }
                Button("Stop", systemImage: "stop.fill") { player.stop() }
            }
            .labelStyle(.iconOnly)
            .buttonStyle(.borderless)
            .padding(.horizontal, 14)
            .padding(.vertical, 8)
            .glassEffect(.regular, in: .capsule)
            .padding(.bottom, 16)
        }
    }
}
