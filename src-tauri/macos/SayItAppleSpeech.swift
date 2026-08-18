import AVFoundation
import Foundation
import Speech

private let modelIdentifier = "apple-speech-transcriber-live"

private struct OutputEvent: Encodable, Sendable {
    let kind: String
    var available: Bool?
    var installed: Bool?
    var locale: String?
    var backend: String?
    var authorization: String?
    var message: String?
    var model: String?
    var text: String?
    var isFinal: Bool?
    var onDevice: Bool?

    enum CodingKeys: String, CodingKey {
        case kind, available, installed, locale, backend, authorization, message, model, text, onDevice
        case isFinal = "final"
    }
}

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

private enum SpeechHelperError: LocalizedError {
    case unavailable
    case unsupportedLocale(String)
    case invalidAudioFormat
    case invalidSampleRate
    case systemAssetsUnavailable(String)
    case authorizationDenied
    case authorizationRestricted

    var errorDescription: String? {
        switch self {
        case .unavailable:
            return "当前设备或系统语言不支持 Apple 纯本地语音识别"
        case .unsupportedLocale(let locale):
            return "Apple 本地语音识别不支持语言 \(locale)"
        case .invalidAudioFormat:
            return "无法创建 Apple 本地语音识别音频格式"
        case .invalidSampleRate:
            return "输入采样率无效"
        case .systemAssetsUnavailable(let locale):
            return "Apple 系统语音资源 \(locale) 尚未就绪"
        case .authorizationDenied:
            return "语音识别权限已被拒绝，请在系统设置的“隐私与安全性”中允许“说吧！”使用语音识别"
        case .authorizationRestricted:
            return "当前系统限制了语音识别权限"
        }
    }
}

private enum LegacyRecognitionEvent: Sendable {
    case result(String, Bool)
    case failure(String)
}

private func authorizationName(_ status: SFSpeechRecognizerAuthorizationStatus) -> String {
    switch status {
    case .authorized:
        return "authorized"
    case .denied:
        return "denied"
    case .restricted:
        return "restricted"
    case .notDetermined:
        return "notDetermined"
    @unknown default:
        return "unknown"
    }
}

private func requestedLocale(_ identifier: String) -> Locale {
    identifier.isEmpty ? Locale.current : Locale(identifier: identifier)
}

private func legacyRecognizer(localeIdentifier: String) -> SFSpeechRecognizer? {
    SFSpeechRecognizer(locale: requestedLocale(localeIdentifier))
}

private func legacyStatus(localeIdentifier: String) -> OutputEvent {
    let authorization = SFSpeechRecognizer.authorizationStatus()
    guard let recognizer = legacyRecognizer(localeIdentifier: localeIdentifier),
          recognizer.supportsOnDeviceRecognition else {
        return OutputEvent(
            kind: "status",
            available: false,
            installed: false,
            authorization: authorizationName(authorization),
            message: SpeechHelperError.unavailable.localizedDescription,
            onDevice: true
        )
    }
    let message: String?
    switch authorization {
    case .denied:
        message = SpeechHelperError.authorizationDenied.localizedDescription
    case .restricted:
        message = SpeechHelperError.authorizationRestricted.localizedDescription
    default:
        message = recognizer.isAvailable ? nil : "Apple 系统语音识别服务当前不可用"
    }
    return OutputEvent(
        kind: "status",
        available: recognizer.isAvailable,
        installed: true,
        locale: recognizer.locale.identifier(.bcp47),
        backend: "SFSpeechRecognizer",
        authorization: authorizationName(authorization),
        message: message,
        onDevice: true
    )
}

private func requestLegacyAuthorization() async throws {
    var authorization = authorizationName(SFSpeechRecognizer.authorizationStatus())
    if authorization == "notDetermined" {
        authorization = await withCheckedContinuation { continuation in
            SFSpeechRecognizer.requestAuthorization { status in
                continuation.resume(returning: authorizationName(status))
            }
        }
    }
    switch authorization {
    case "authorized":
        return
    case "denied":
        throw SpeechHelperError.authorizationDenied
    default:
        throw SpeechHelperError.authorizationRestricted
    }
}

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

private func transcribeLegacy(localeIdentifier: String, sampleRate: Double) async -> Int32 {
    let emitter = JsonEmitter()
    do {
        guard sampleRate.isFinite, sampleRate > 0 else {
            throw SpeechHelperError.invalidSampleRate
        }
        try await requestLegacyAuthorization()
        guard let recognizer = legacyRecognizer(localeIdentifier: localeIdentifier),
              recognizer.supportsOnDeviceRecognition,
              recognizer.isAvailable else {
            throw SpeechHelperError.unavailable
        }
        guard let sourceFormat = AVAudioFormat(
            commonFormat: .pcmFormatFloat32,
            sampleRate: sampleRate,
            channels: 1,
            interleaved: false
        ) else {
            throw SpeechHelperError.invalidAudioFormat
        }

        let request = SFSpeechAudioBufferRecognitionRequest()
        request.shouldReportPartialResults = true
        request.requiresOnDeviceRecognition = true
        request.addsPunctuation = true
        let (events, eventBuilder) = AsyncStream.makeStream(of: LegacyRecognitionEvent.self)
        let recognitionTask = recognizer.recognitionTask(with: request) { result, error in
            if let result {
                let text = result.bestTranscription.formattedString
                if !text.isEmpty {
                    eventBuilder.yield(.result(text, result.isFinal))
                }
            }
            if let error {
                eventBuilder.yield(.failure(error.localizedDescription))
                eventBuilder.finish()
            } else if result?.isFinal == true {
                eventBuilder.finish()
            }
        }
        let resultTask = Task { () -> String? in
            for await event in events {
                switch event {
                case .result(let text, let isFinal):
                    await emitter.send(OutputEvent(
                        kind: "result",
                        text: text,
                        isFinal: isFinal
                    ))
                case .failure(let message):
                    return message
                }
            }
            return nil
        }

        await emitter.send(OutputEvent(
            kind: "opened",
            locale: recognizer.locale.identifier(.bcp47),
            backend: "SFSpeechRecognizer",
            authorization: "authorized",
            model: modelIdentifier,
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
            request.append(try makeBuffer(data: Data(complete), format: sourceFormat))
        }
        request.endAudio()
        let timeoutTask = Task {
            try? await Task.sleep(nanoseconds: 15_000_000_000)
            guard !Task.isCancelled else { return }
            eventBuilder.yield(.failure("Apple 本地语音识别结束等待超时"))
            eventBuilder.finish()
        }
        if let resultError = await resultTask.value {
            timeoutTask.cancel()
            recognitionTask.cancel()
            throw NSError(
                domain: "com.henjicc.sayit.apple-speech",
                code: 2,
                userInfo: [NSLocalizedDescriptionKey: resultError]
            )
        }
        timeoutTask.cancel()
        await emitter.send(OutputEvent(kind: "finish"))
        return 0
    } catch {
        await emitter.send(OutputEvent(kind: "error", message: error.localizedDescription))
        return 1
    }
}

#if SAYIT_HAS_SPEECH_ANALYZER
@available(macOS 26.0, *)
private func resolvedAnalyzerLocale(_ identifier: String) async throws -> Locale {
    guard SpeechTranscriber.isAvailable else {
        throw SpeechHelperError.unavailable
    }
    let requested = requestedLocale(identifier)
    guard let supported = await SpeechTranscriber.supportedLocale(equivalentTo: requested) else {
        throw SpeechHelperError.unsupportedLocale(requested.identifier)
    }
    return supported
}

@available(macOS 26.0, *)
private func analyzerIsInstalled(_ locale: Locale) async -> Bool {
    let identifier = locale.identifier(.bcp47)
    let installedLocales = await SpeechTranscriber.installedLocales
    return installedLocales.contains {
        $0.identifier(.bcp47) == identifier
    }
}

@available(macOS 26.0, *)
private func analyzerStatus(localeIdentifier: String) async -> OutputEvent {
    do {
        let locale = try await resolvedAnalyzerLocale(localeIdentifier)
        return OutputEvent(
            kind: "status",
            available: true,
            installed: await analyzerIsInstalled(locale),
            locale: locale.identifier(.bcp47),
            backend: "SpeechAnalyzer",
            authorization: "notRequired",
            onDevice: true
        )
    } catch {
        return OutputEvent(
            kind: "status",
            available: false,
            installed: false,
            authorization: "notRequired",
            message: error.localizedDescription,
            onDevice: true
        )
    }
}

@available(macOS 26.0, *)
private func prepareAnalyzer(localeIdentifier: String) async -> Int32 {
    let emitter = JsonEmitter()
    do {
        let locale = try await resolvedAnalyzerLocale(localeIdentifier)
        let transcriber = SpeechTranscriber(locale: locale, preset: .progressiveTranscription)
        if let request = try await AssetInventory.assetInstallationRequest(supporting: [transcriber]) {
            await emitter.send(OutputEvent(
                kind: "preparing",
                locale: locale.identifier(.bcp47),
                backend: "SpeechAnalyzer",
                message: "正在由 macOS 准备本地语音识别资源"
            ))
            try await request.downloadAndInstall()
        }
        let status = await analyzerStatus(localeIdentifier: locale.identifier(.bcp47))
        await emitter.send(status)
        return 0
    } catch {
        await emitter.send(OutputEvent(kind: "error", message: error.localizedDescription))
        return 1
    }
}

@available(macOS 26.0, *)
private func transcribeAnalyzer(localeIdentifier: String, sampleRate: Double) async -> Int32 {
    let emitter = JsonEmitter()
    do {
        guard sampleRate.isFinite, sampleRate > 0 else {
            throw SpeechHelperError.invalidSampleRate
        }
        let locale = try await resolvedAnalyzerLocale(localeIdentifier)
        let transcriber = SpeechTranscriber(locale: locale, preset: .progressiveTranscription)
        if !(await analyzerIsInstalled(locale)) {
            throw SpeechHelperError.systemAssetsUnavailable(locale.identifier(.bcp47))
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
            authorization: "notRequired",
            model: modelIdentifier,
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
#endif

private func probe(localeIdentifier: String) async -> Int32 {
    let emitter = JsonEmitter()
#if SAYIT_HAS_SPEECH_ANALYZER
    if #available(macOS 26.0, *) {
        let status = await analyzerStatus(localeIdentifier: localeIdentifier)
        if status.available == true {
            await emitter.send(status)
            return 0
        }
    }
#endif
    let status = legacyStatus(localeIdentifier: localeIdentifier)
    await emitter.send(status)
    return status.available == true ? 0 : 2
}

private func prepare(localeIdentifier: String) async -> Int32 {
#if SAYIT_HAS_SPEECH_ANALYZER
    if #available(macOS 26.0, *) {
        let status = await analyzerStatus(localeIdentifier: localeIdentifier)
        if status.available == true {
            return await prepareAnalyzer(localeIdentifier: localeIdentifier)
        }
    }
#endif
    let emitter = JsonEmitter()
    let status = legacyStatus(localeIdentifier: localeIdentifier)
    await emitter.send(status)
    return status.available == true ? 0 : 2
}

private func transcribe(localeIdentifier: String, sampleRate: Double) async -> Int32 {
#if SAYIT_HAS_SPEECH_ANALYZER
    if #available(macOS 26.0, *) {
        let status = await analyzerStatus(localeIdentifier: localeIdentifier)
        if status.available == true {
            return await transcribeAnalyzer(localeIdentifier: localeIdentifier, sampleRate: sampleRate)
        }
    }
#endif
    return await transcribeLegacy(localeIdentifier: localeIdentifier, sampleRate: sampleRate)
}

@main
private struct SayItAppleSpeechHelper {
    static func main() async {
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
