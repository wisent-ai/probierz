import AVFoundation
import CoreMedia
import Foundation
import ScreenCaptureKit

private enum CaptureError: LocalizedError {
    case usage
    case appWindowNotFound(String)
    case writer(String)
    case noFrames

    var errorDescription: String? {
        switch self {
        case .usage:
            "usage: screen-capture-kit --bundle-id <id> --output <file.mp4> [--wait-seconds <n>]"
        case let .appWindowNotFound(bundleID):
            "no capturable window appeared for \(bundleID)"
        case let .writer(message):
            "asset writer failed: \(message)"
        case .noFrames:
            "capture stopped before any complete frame was recorded"
        }
    }
}

private final class SignalWaiter: @unchecked Sendable {
    private var sources: [DispatchSourceSignal] = []

    func wait() async {
        await withCheckedContinuation { continuation in
            let lock = NSLock()
            var resumed = false
            for value in [SIGINT, SIGTERM] {
                signal(value, SIG_IGN)
                let source = DispatchSource.makeSignalSource(signal: value, queue: .global(qos: .userInitiated))
                source.setEventHandler {
                    lock.lock()
                    defer { lock.unlock() }
                    guard !resumed else { return }
                    resumed = true
                    continuation.resume()
                }
                source.resume()
                sources.append(source)
            }
        }
    }
}

@available(macOS 13.0, *)
private final class WindowRecorder: NSObject, SCStreamOutput, @unchecked Sendable {
    private let writer: AVAssetWriter
    private let input: AVAssetWriterInput
    private let queue = DispatchQueue(label: "ai.wisent.probierz.screen-capture")
    private var stream: SCStream?
    private var started = false
    private var stopping = false
    private(set) var frameCount = 0

    init(output: URL, width: Int, height: Int) throws {
        try? FileManager.default.removeItem(at: output)
        writer = try AVAssetWriter(outputURL: output, fileType: .mp4)
        input = AVAssetWriterInput(
            mediaType: .video,
            outputSettings: [
                AVVideoCodecKey: AVVideoCodecType.h264,
                AVVideoWidthKey: width,
                AVVideoHeightKey: height,
                AVVideoCompressionPropertiesKey: [
                    AVVideoAverageBitRateKey: max(2_000_000, width * height * 4),
                    AVVideoExpectedSourceFrameRateKey: 30,
                    AVVideoMaxKeyFrameIntervalKey: 60,
                ],
            ]
        )
        input.expectsMediaDataInRealTime = true
        guard writer.canAdd(input) else { throw CaptureError.writer("video input rejected") }
        writer.add(input)
    }

    func start(window: SCWindow) async throws {
        let width = max(2, Int(window.frame.width.rounded(.up)))
        let height = max(2, Int(window.frame.height.rounded(.up)))
        let configuration = SCStreamConfiguration()
        configuration.width = width.isMultiple(of: 2) ? width : width + 1
        configuration.height = height.isMultiple(of: 2) ? height : height + 1
        configuration.minimumFrameInterval = CMTime(value: 1, timescale: 30)
        configuration.queueDepth = 6
        configuration.pixelFormat = kCVPixelFormatType_32BGRA
        configuration.showsCursor = false
        configuration.capturesAudio = false

        let stream = SCStream(
            filter: SCContentFilter(desktopIndependentWindow: window),
            configuration: configuration,
            delegate: nil
        )
        try stream.addStreamOutput(self, type: .screen, sampleHandlerQueue: queue)
        self.stream = stream
        try await stream.startCapture()
    }

    func stream(
        _ stream: SCStream,
        didOutputSampleBuffer sampleBuffer: CMSampleBuffer,
        of outputType: SCStreamOutputType
    ) {
        guard outputType == .screen, sampleBuffer.isValid else { return }
        guard let attachments = CMSampleBufferGetSampleAttachmentsArray(sampleBuffer, createIfNecessary: false)
            as? [[SCStreamFrameInfo: Any]],
            let statusValue = attachments.first?[.status] as? Int,
            SCFrameStatus(rawValue: statusValue) == .complete
        else { return }
        guard !stopping else { return }
        if !started {
            guard writer.startWriting() else {
                stopping = true
                return
            }
            writer.startSession(atSourceTime: sampleBuffer.presentationTimeStamp)
            started = true
        }
        guard input.isReadyForMoreMediaData else { return }
        if input.append(sampleBuffer) { frameCount += 1 }
    }

    func finish() async throws {
        stopping = true
        if let stream { try await stream.stopCapture() }
        try await queue.asyncResult {
            guard self.started else { throw CaptureError.noFrames }
            self.input.markAsFinished()
        }
        await withCheckedContinuation { continuation in
            writer.finishWriting { continuation.resume() }
        }
        guard writer.status == .completed else {
            throw CaptureError.writer(writer.error?.localizedDescription ?? String(describing: writer.status))
        }
    }
}

private extension DispatchQueue {
    func asyncResult<T: Sendable>(_ work: @escaping @Sendable () throws -> T) async throws -> T {
        try await withCheckedThrowingContinuation { continuation in
            async {
                do { continuation.resume(returning: try work()) }
                catch { continuation.resume(throwing: error) }
            }
        }
    }
}

@available(macOS 13.0, *)
private func waitForWindow(bundleID: String, seconds: Int) async throws -> SCWindow {
    let attempts = max(1, seconds * 2)
    for _ in 0..<attempts {
        let content = try await SCShareableContent.excludingDesktopWindows(true, onScreenWindowsOnly: false)
        let applications = Set(
            content.applications
                .filter { $0.bundleIdentifier == bundleID }
                .map(\.processID)
        )
        if let window = content.windows
            .filter({ window in
                guard let app = window.owningApplication else { return false }
                return applications.contains(app.processID)
                    && window.frame.width >= 200
                    && window.frame.height >= 120
            })
            .max(by: { $0.frame.width * $0.frame.height < $1.frame.width * $1.frame.height }) {
            return window
        }
        try await Task.sleep(for: .milliseconds(500))
    }
    throw CaptureError.appWindowNotFound(bundleID)
}

@main
private enum ScreenCaptureKitRecorder {
    static func main() async {
        do {
            let arguments = Array(CommandLine.arguments.dropFirst())
            guard let bundleIndex = arguments.firstIndex(of: "--bundle-id"),
                  arguments.indices.contains(bundleIndex + 1),
                  let outputIndex = arguments.firstIndex(of: "--output"),
                  arguments.indices.contains(outputIndex + 1)
            else { throw CaptureError.usage }
            let bundleID = arguments[bundleIndex + 1]
            let output = URL(fileURLWithPath: arguments[outputIndex + 1])
            let waitSeconds: Int
            if let waitIndex = arguments.firstIndex(of: "--wait-seconds"),
               arguments.indices.contains(waitIndex + 1) {
                waitSeconds = Int(arguments[waitIndex + 1]) ?? 60
            } else {
                waitSeconds = 60
            }
            try FileManager.default.createDirectory(
                at: output.deletingLastPathComponent(),
                withIntermediateDirectories: true
            )
            let window = try await waitForWindow(bundleID: bundleID, seconds: waitSeconds)
            let width = max(2, Int(window.frame.width.rounded(.up)))
            let height = max(2, Int(window.frame.height.rounded(.up)))
            let recorder = try WindowRecorder(
                output: output,
                width: width.isMultiple(of: 2) ? width : width + 1,
                height: height.isMultiple(of: 2) ? height : height + 1
            )
            try await recorder.start(window: window)
            await SignalWaiter().wait()
            try await recorder.finish()
            let result = ["file": output.path, "frames": String(recorder.frameCount)]
            let data = try JSONSerialization.data(withJSONObject: result, options: [.sortedKeys])
            FileHandle.standardOutput.write(data)
            FileHandle.standardOutput.write(Data("\n".utf8))
        } catch {
            FileHandle.standardError.write(Data("screen-capture-kit: \(error.localizedDescription)\n".utf8))
            Foundation.exit(EXIT_FAILURE)
        }
    }
}
