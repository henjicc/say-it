#if SAYIT_HAS_SPEECH_ANALYZER
import AVFoundation
import Foundation
import Speech

@available(macOS 26.0, *)
private struct OutputEvent: Encodable, Sendable {
    let kind: String
    var available: Bool?
    var installed: Bool?
    var locale: String?
    var backend: String?
    var message: String?
    var model: String?
    var text: String?
    var isFinal: Bool?
    var onDevice: Bool?

    enum CodingKeys: String, CodingKey {
        case kind, available, installed, locale, backend, message, model, text, onDevice
        case isFinal = "final"
    }
}

@available(macOS 26.0, *)
private actor JsonEmitter {
    func send(_ event: OutputEvent) {
        guard let data = try? JSONEncoder().encode(event),
              var line = String(data: data, encoding: .utf8) else {
            return
        }
        line.append("\n")
        FileHandle.standardOutput.write(Data(line.utf8))
    }
}

@available(macOS 26.0, *)
private enum SpeechHelperError: LocalizedError {
    case unavailable
    case unsupportedLocale(String)
    case invalidAudioFormat
    case invalidSampleRate
    case modelNotInstalled(String)

    var errorDescription: String? {
        switch self {
        case .unavailable:
            return "当前设备不支持 Apple SpeechTranscriber"
        case .unsupportedLocale(let locale):
            return "Apple 本地语音识别不支持语言 \(locale)"
        case .invalidAudioFormat:
            return "无法创建 Apple 本地语音识别音频格式"
        case .invalidSampleRate:
            return "输入采样率无效"
        case .modelNotInstalled(let locale):
            return "Apple 本地语音模型 \(locale) 尚未安装，请先在设置中下载"
        }
    }
}

@available(macOS 26.0, *)
private func prepare(localeIdentifier: String) async -> Int32 {
    let emitter = JsonEmitter()
    do {
        let locale = try await resolvedLocale(localeIdentifier)
        let transcriber = SpeechTranscriber(locale: locale, preset: .progressiveTranscription)
        if let request = try await AssetInventory.assetInstallationRequest(supporting: [transcriber]) {
            await emitter.send(OutputEvent(
                kind: "preparing",
                locale: locale.identifier(.bcp47),
                message: "正在下载并安装 Apple 本地语音模型"
            ))
            try await request.downloadAndInstall()
        }
        await emitter.send(OutputEvent(
            kind: "status",
            available: true,
            installed: await isInstalled(locale),
            locale: locale.identifier(.bcp47),
            backend: "SpeechAnalyzer"
        ))
        return 0
    } catch {
        await emitter.send(OutputEvent(kind: "error", message: error.localizedDescription))
        return 1
    }
}

@available(macOS 26.0, *)
private func resolvedLocale(_ identifier: String) async throws -> Locale {
    guard SpeechTranscriber.isAvailable else {
        throw SpeechHelperError.unavailable
    }
    let requested = identifier.isEmpty ? Locale.current : Locale(identifier: identifier)
    guard let supported = await SpeechTranscriber.supportedLocale(equivalentTo: requested) else {
        throw SpeechHelperError.unsupportedLocale(requested.identifier)
    }
    return supported
}

@available(macOS 26.0, *)
private func isInstalled(_ locale: Locale) async -> Bool {
    let identifier = locale.identifier(.bcp47)
    let installedLocales = await SpeechTranscriber.installedLocales
    return installedLocales.contains {
        $0.identifier(.bcp47) == identifier
    }
}

@available(macOS 26.0, *)
private func probe(localeIdentifier: String) async -> Int32 {
    let emitter = JsonEmitter()
    do {
        let locale = try await resolvedLocale(localeIdentifier)
        await emitter.send(OutputEvent(
            kind: "status",
            available: true,
            installed: await isInstalled(locale),
            locale: locale.identifier(.bcp47),
            backend: "SpeechAnalyzer"
        ))
        return 0
    } catch {
        await emitter.send(OutputEvent(
            kind: "status",
            available: false,
            installed: false,
            message: error.localizedDescription
        ))
        return 2
    }
}

@available(macOS 26.0, *)
private func makeBuffer(data: Data, format: AVAudioFormat) throws -> AVAudioPCMBuffer {
    let frameCount = data.count / MemoryLayout<Float>.size
    guard frameCount > 0,
          let buffer = AVAudioPCMBuffer(
              pcmFormat: format,
              frameCapacity: AVAudioFrameCount(frameCount)
          ),
          let channel = buffer.floatChannelData?[0] else {
        throw SpeechHelperError.invalidAudioFormat
    }
    let destination = UnsafeMutableRawBufferPointer(start: channel, count: data.count)
    _ = data.copyBytes(to: destination)
    buffer.frameLength = AVAudioFrameCount(frameCount)
    return buffer
}

@available(macOS 26.0, *)
private func transcribe(localeIdentifier: String, sampleRate: Double) async -> Int32 {
    let emitter = JsonEmitter()
    do {
        guard sampleRate.isFinite, sampleRate > 0 else {
            throw SpeechHelperError.invalidSampleRate
        }
        let locale = try await resolvedLocale(localeIdentifier)
        let transcriber = SpeechTranscriber(locale: locale, preset: .progressiveTranscription)
        if !(await isInstalled(locale)) {
            throw SpeechHelperError.modelNotInstalled(locale.identifier(.bcp47))
        }

        guard let analyzerFormat = await SpeechAnalyzer.bestAvailableAudioFormat(
            compatibleWith: [transcriber]
        ), let sourceFormat = AVAudioFormat(
            commonFormat: .pcmFormatFloat32,
            sampleRate: sampleRate,
            channels: 1,
            interleaved: false
        ) else {
            throw SpeechHelperError.invalidAudioFormat
        }

        let analyzer = SpeechAnalyzer(modules: [transcriber])
        try await analyzer.prepareToAnalyze(in: analyzerFormat)
        let converter = AnalyzerInputConverter(analyzerFormat: analyzerFormat)
        let (inputSequence, inputBuilder) = AsyncStream.makeStream(of: AnalyzerInput.self)

        let resultTask = Task { () -> String? in
            do {
                for try await result in transcriber.results {
                    let text = String(result.text.characters)
                    if !text.isEmpty {
                        await emitter.send(OutputEvent(
                            kind: "result",
                            text: text,
                            isFinal: result.isFinal
                        ))
                    }
                }
                return nil
            } catch {
                return error.localizedDescription
            }
        }
        async let lastSampleTime = analyzer.analyzeSequence(inputSequence)

        await emitter.send(OutputEvent(
            kind: "opened",
            locale: locale.identifier(.bcp47),
            backend: "SpeechAnalyzer",
            model: "apple-speech-transcriber-live",
            onDevice: true
        ))

        var pending = Data()
        while true {
            let chunk = FileHandle.standardInput.readData(ofLength: 32 * 1024)
            if chunk.isEmpty { break }
            pending.append(chunk)
            let usableCount = pending.count - pending.count % MemoryLayout<Float>.size
            if usableCount == 0 { continue }
            let complete = pending.prefix(usableCount)
            pending.removeFirst(usableCount)
            let buffer = try makeBuffer(data: Data(complete), format: sourceFormat)
            for input in try converter.convert(buffer, at: nil) {
                inputBuilder.yield(input)
            }
        }
        for input in try converter.flush() {
            inputBuilder.yield(input)
        }
        inputBuilder.finish()

        if let lastSampleTime = try await lastSampleTime {
            try await analyzer.finalizeAndFinish(through: lastSampleTime)
        } else {
            await analyzer.cancelAndFinishNow()
        }
        if let resultError = await resultTask.value {
            throw NSError(
                domain: "com.henjicc.sayit.apple-speech",
                code: 1,
                userInfo: [NSLocalizedDescriptionKey: resultError]
            )
        }
        await emitter.send(OutputEvent(kind: "finish"))
        return 0
    } catch {
        await emitter.send(OutputEvent(kind: "error", message: error.localizedDescription))
        return 1
    }
}

@main
private struct SayItAppleSpeechHelper {
    static func main() async {
        guard #available(macOS 26.0, *) else {
            fputs("{\"kind\":\"status\",\"available\":false,\"installed\":false,\"message\":\"需要 macOS 26 或更高版本\"}\n", stdout)
            Foundation.exit(2)
        }
        let arguments = CommandLine.arguments
        let locale = value(after: "--locale", in: arguments) ?? ""
        if arguments.contains("--probe") {
            Foundation.exit(await probe(localeIdentifier: locale))
        }
        if arguments.contains("--prepare") {
            Foundation.exit(await prepare(localeIdentifier: locale))
        }
        let sampleRate = Double(value(after: "--sample-rate", in: arguments) ?? "") ?? 0
        Foundation.exit(await transcribe(localeIdentifier: locale, sampleRate: sampleRate))
    }

    private static func value(after key: String, in arguments: [String]) -> String? {
        guard let index = arguments.firstIndex(of: key), arguments.indices.contains(index + 1) else {
            return nil
        }
        return arguments[index + 1]
    }
}
#else
import Foundation

@main
private struct SayItAppleSpeechUnavailableHelper {
    static func main() {
        print("{\"kind\":\"status\",\"available\":false,\"installed\":false,\"message\":\"构建工具链缺少 macOS 26 SpeechAnalyzer SDK\"}")
        Foundation.exit(2)
    }
}
#endif
