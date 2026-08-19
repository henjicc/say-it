import AVFoundation
import Darwin
import Foundation
import Speech

private let modelIdentifier = "apple-speech-transcriber-live"
#if SAYIT_DEVELOPMENT_BUNDLE
private let expectedBundleIdentifier = "com.henjicc.sayit.dev.apple-speech"
#else
private let expectedBundleIdentifier = "com.henjicc.sayit"
#endif

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
    var identityValid: Bool?
    var bundleIdentifier: String?
    var usageDescriptionPresent: Bool?
    var processId: Int32?

    enum CodingKeys: String, CodingKey {
        case kind, available, installed, locale, backend, authorization, message, model, text, onDevice
        case identityValid, bundleIdentifier, usageDescriptionPresent
        case processId
        case isFinal = "final"
    }
}

private actor JsonEmitter {
    private let output: FileHandle

    init(output: FileHandle = .standardOutput) {
        self.output = output
    }

    func send(_ event: OutputEvent) {
        guard let data = try? JSONEncoder().encode(event),
              var line = String(data: data, encoding: .utf8) else {
            return
        }
        line.append("\n")
        output.write(Data(line.utf8))
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
    case invalidBundleMetadata

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
        case .invalidBundleMetadata:
            return "Apple 语音识别助手缺少 macOS 权限身份，请重新构建开发版或重新安装应用"
        }
    }
}

private struct BundleMetadata {
    let identifier: String
    let usageDescriptionPresent: Bool

    var isValid: Bool {
        identifier == expectedBundleIdentifier && usageDescriptionPresent
    }
}

private func bundleMetadata() -> BundleMetadata {
    let identifier = Bundle.main.bundleIdentifier ?? ""
    let usageDescription = Bundle.main.object(
        forInfoDictionaryKey: "NSSpeechRecognitionUsageDescription"
    ) as? String
    return BundleMetadata(
        identifier: identifier,
        usageDescriptionPresent: !(usageDescription?.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ?? true)
    )
}

private func withBundleMetadata(_ event: OutputEvent) -> OutputEvent {
    let metadata = bundleMetadata()
    var enriched = event
    enriched.identityValid = metadata.isValid
    enriched.bundleIdentifier = metadata.identifier
    enriched.usageDescriptionPresent = metadata.usageDescriptionPresent
    return enriched
}

private func selfCheck() async -> Int32 {
    let metadata = bundleMetadata()
    let emitter = JsonEmitter()
    await emitter.send(OutputEvent(
        kind: "selfCheck",
        message: metadata.isValid ? nil : SpeechHelperError.invalidBundleMetadata.localizedDescription,
        identityValid: metadata.isValid,
        bundleIdentifier: metadata.identifier,
        usageDescriptionPresent: metadata.usageDescriptionPresent
    ))
    return metadata.isValid ? 0 : 78
}

private func emitInvalidIdentity() async -> Int32 {
    let metadata = bundleMetadata()
    let emitter = JsonEmitter()
    await emitter.send(OutputEvent(
        kind: "error",
        message: SpeechHelperError.invalidBundleMetadata.localizedDescription,
        identityValid: false,
        bundleIdentifier: metadata.identifier,
        usageDescriptionPresent: metadata.usageDescriptionPresent
    ))
    return 78
}

private enum LegacyRecognitionEvent: Sendable {
    case result(String, Bool, TimeInterval?)
    case failure(String)
}

/// Apple 的旧识别器在长会话中会悄悄滚动内部识别窗口：新的 partial 只包含
/// 后半段，但不会先为旧窗口发送 final。这里把每个窗口的最后快照保留下来，
/// 对外仍维持“本次会话完整快照”的协议。
private struct LegacyTranscriptAccumulator {
    private(set) var committed = ""
    private(set) var current = ""
    private var rangeStart: TimeInterval?

    mutating func update(
        snapshot: String,
        rangeStart nextRangeStart: TimeInterval?
    ) -> String {
        let next = snapshot.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !next.isEmpty else { return text }
        let timelineRolled = rangeStart.flatMap { previous in
            nextRangeStart.map { $0 > previous + 0.75 }
        } ?? false
        // 部分系统版本在滚动后会把 segment 时间重新从零计算；文本突然缩短是
        // 这一路径的兜底信号。小幅回改仍只替换 current，不会被错误提交。
        let sharedPrefixCount = zip(current, next).prefix { $0 == $1 }.count
        let sameWindowPrefix = min(8, max(1, next.count / 2))
        let severeShrink = current.count >= 24
            && next.count * 3 < current.count
            && sharedPrefixCount < sameWindowPrefix
        if !current.isEmpty && (timelineRolled || severeShrink) {
            committed = concatenateTranscript(committed, current)
        }
        current = next
        rangeStart = nextRangeStart
        return text
    }

    var text: String {
        concatenateTranscript(committed, current)
    }
}

/// SpeechAnalyzer 返回的是按音频范围排列的 phrase，而不是整段听写快照。
/// final phrase 依次追加，volatile phrase 只替换当前尾部。
private struct ProgressiveTranscriptAccumulator {
    private(set) var finalized = ""
    private(set) var volatile = ""

    mutating func update(text: String, isFinal: Bool) -> String {
        if isFinal {
            // Apple 的范围结果自带边界空白和标点，必须像官方示例一样原样拼接。
            finalized += text
            volatile = ""
        } else {
            volatile = text
        }
        return self.text
    }

    var text: String {
        finalized + volatile
    }
}

private func concatenateTranscript(_ left: String, _ right: String) -> String {
    guard !left.isEmpty else { return right }
    guard !right.isEmpty else { return left }
    guard let last = left.unicodeScalars.last,
          let first = right.unicodeScalars.first else {
        return left + right
    }
    let needsSpace = last.isASCII
        && first.isASCII
        && CharacterSet.alphanumerics.contains(last)
        && CharacterSet.alphanumerics.contains(first)
    return left + (needsSpace ? " " : "") + right
}

private func accumulatorCheck() async -> Int32 {
    var legacy = LegacyTranscriptAccumulator()
    _ = legacy.update(snapshot: "第一段临时文本", rangeStart: 0)
    _ = legacy.update(snapshot: "第一段修正文本", rangeStart: 0)
    let legacyResult = legacy.update(snapshot: "第二段", rangeStart: 61)

    var progressive = ProgressiveTranscriptAccumulator()
    _ = progressive.update(text: "第一段临时", isFinal: false)
    _ = progressive.update(text: "第一段", isFinal: true)
    _ = progressive.update(text: "第二", isFinal: false)
    let progressiveResult = progressive.update(text: "第二段", isFinal: true)

    let success = legacyResult == "第一段修正文本第二段"
        && progressiveResult == "第一段第二段"
    let emitter = JsonEmitter()
    await emitter.send(OutputEvent(
        kind: "accumulatorCheck",
        available: success,
        message: success ? nil : "Apple 转写累加器自检失败"
    ))
    return success ? 0 : 70
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
    let locale = requestedLocale(localeIdentifier)
    if authorization == .notDetermined {
        // 在真正开始听写前不能创建 recognizer 或查询设备资源；部分 macOS 版本会把
        // 这种“未授权预检”直接记成 denied，导致用户永远看不到系统授权弹窗。
        return withBundleMetadata(OutputEvent(
            kind: "status",
            available: true,
            installed: true,
            locale: locale.identifier(.bcp47),
            backend: "SFSpeechRecognizer",
            authorization: authorizationName(authorization),
            onDevice: true
        ))
    }
    if authorization == .denied || authorization == .restricted {
        return withBundleMetadata(OutputEvent(
            kind: "status",
            available: false,
            installed: false,
            locale: locale.identifier(.bcp47),
            backend: "SFSpeechRecognizer",
            authorization: authorizationName(authorization),
            message: authorization == .denied
                ? SpeechHelperError.authorizationDenied.localizedDescription
                : SpeechHelperError.authorizationRestricted.localizedDescription,
            onDevice: true
        ))
    }
    guard let recognizer = legacyRecognizer(localeIdentifier: localeIdentifier) else {
        return withBundleMetadata(OutputEvent(
            kind: "status",
            available: false,
            installed: false,
            authorization: authorizationName(authorization),
            message: SpeechHelperError.unavailable.localizedDescription,
            onDevice: true
        ))
    }
    guard recognizer.supportsOnDeviceRecognition else {
        return withBundleMetadata(OutputEvent(
            kind: "status",
            available: false,
            installed: false,
            locale: recognizer.locale.identifier(.bcp47),
            backend: "SFSpeechRecognizer",
            authorization: authorizationName(authorization),
            message: SpeechHelperError.unavailable.localizedDescription,
            onDevice: true
        ))
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
    return withBundleMetadata(OutputEvent(
        kind: "status",
        available: recognizer.isAvailable,
        installed: true,
        locale: recognizer.locale.identifier(.bcp47),
        backend: "SFSpeechRecognizer",
        authorization: authorizationName(authorization),
        message: message,
        onDevice: true
    ))
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

private func authorize() async -> Int32 {
    let emitter = JsonEmitter()
#if SAYIT_HAS_SPEECH_ANALYZER
    if #available(macOS 26.0, *) {
        await emitter.send(withBundleMetadata(OutputEvent(
            kind: "status",
            available: true,
            installed: true,
            backend: "SpeechAnalyzer",
            authorization: "notRequired",
            onDevice: true
        )))
        return 0
    }
#endif
    do {
        try await requestLegacyAuthorization()
        await emitter.send(withBundleMetadata(OutputEvent(
            kind: "status",
            available: true,
            installed: true,
            backend: "SFSpeechRecognizer",
            authorization: "authorized",
            onDevice: true
        )))
        return 0
    } catch {
        await emitter.send(withBundleMetadata(OutputEvent(
            kind: "error",
            authorization: authorizationName(SFSpeechRecognizer.authorizationStatus()),
            message: error.localizedDescription,
            onDevice: true
        )))
        return 1
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

private func transcribeLegacy(
    localeIdentifier: String,
    sampleRate: Double,
    input: FileHandle,
    output: FileHandle
) async -> Int32 {
    let emitter = JsonEmitter(output: output)
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
                    let segments = result.bestTranscription.segments
                    let rangeStart = segments.first?.timestamp
                    eventBuilder.yield(.result(text, result.isFinal, rangeStart))
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
            var accumulator = LegacyTranscriptAccumulator()
            for await event in events {
                switch event {
                case .result(let text, let isFinal, let rangeStart):
                    let snapshot = accumulator.update(
                        snapshot: text,
                        rangeStart: rangeStart
                    )
                    await emitter.send(OutputEvent(
                        kind: "result",
                        text: snapshot,
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
            let chunk = input.readData(ofLength: 32 * 1024)
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
        return withBundleMetadata(OutputEvent(
            kind: "status",
            available: true,
            installed: await analyzerIsInstalled(locale),
            locale: locale.identifier(.bcp47),
            backend: "SpeechAnalyzer",
            authorization: "notRequired",
            onDevice: true
        ))
    } catch {
        return withBundleMetadata(OutputEvent(
            kind: "status",
            available: false,
            installed: false,
            authorization: "notRequired",
            message: error.localizedDescription,
            onDevice: true
        ))
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
private func transcribeAnalyzer(
    localeIdentifier: String,
    sampleRate: Double,
    input: FileHandle,
    output: FileHandle
) async -> Int32 {
    let emitter = JsonEmitter(output: output)
    do {
        guard sampleRate.isFinite, sampleRate > 0 else {
            throw SpeechHelperError.invalidSampleRate
        }
        let locale = try await resolvedAnalyzerLocale(localeIdentifier)
        let transcriber = SpeechTranscriber(locale: locale, preset: .progressiveTranscription)
        if !(await analyzerIsInstalled(locale)),
           let request = try await AssetInventory.assetInstallationRequest(supporting: [transcriber]) {
            try await request.downloadAndInstall()
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

        let resultTask = Task { () -> (text: String, error: String?) in
            var accumulator = ProgressiveTranscriptAccumulator()
            do {
                for try await result in transcriber.results {
                    let text = String(result.text.characters)
                    let snapshot = accumulator.update(text: text, isFinal: result.isFinal)
                    if !snapshot.isEmpty {
                        await emitter.send(OutputEvent(
                            kind: "result",
                            text: snapshot,
                            // Result.isFinal 只表示当前音频范围已稳定，不代表整个
                            // 听写会话结束；会话 final 在 analyzer 完成后统一发送。
                            isFinal: false
                        ))
                    }
                }
                return (accumulator.text, nil)
            } catch {
                return (accumulator.text, error.localizedDescription)
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
            let chunk = input.readData(ofLength: 32 * 1024)
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
        let result = await resultTask.value
        if let resultError = result.error {
            throw NSError(
                domain: "com.henjicc.sayit.apple-speech",
                code: 1,
                userInfo: [NSLocalizedDescriptionKey: resultError]
            )
        }
        if !result.text.isEmpty {
            await emitter.send(OutputEvent(
                kind: "result",
                text: result.text,
                isFinal: true
            ))
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

private func transcribe(
    localeIdentifier: String,
    sampleRate: Double,
    input: FileHandle,
    output: FileHandle
) async -> Int32 {
#if SAYIT_HAS_SPEECH_ANALYZER
    if #available(macOS 26.0, *) {
        let status = await analyzerStatus(localeIdentifier: localeIdentifier)
        if status.available == true {
            return await transcribeAnalyzer(
                localeIdentifier: localeIdentifier,
                sampleRate: sampleRate,
                input: input,
                output: output
            )
        }
    }
#endif
    return await transcribeLegacy(
        localeIdentifier: localeIdentifier,
        sampleRate: sampleRate,
        input: input,
        output: output
    )
}

private func connectUnixSocket(path: String) throws -> FileHandle {
    let pathBytes = Array(path.utf8)
    var address = sockaddr_un()
    let capacity = MemoryLayout.size(ofValue: address.sun_path)
    guard !pathBytes.isEmpty, pathBytes.count + 1 <= capacity else {
        throw NSError(
            domain: "com.henjicc.sayit.apple-speech",
            code: 3,
            userInfo: [NSLocalizedDescriptionKey: "开发语音通道路径无效"]
        )
    }
    let descriptor = Darwin.socket(AF_UNIX, SOCK_STREAM, 0)
    guard descriptor >= 0 else {
        throw NSError(
            domain: NSPOSIXErrorDomain,
            code: Int(errno),
            userInfo: [NSLocalizedDescriptionKey: "创建开发语音通道失败"]
        )
    }
    address.sun_family = sa_family_t(AF_UNIX)
    let addressLength = MemoryLayout<sa_family_t>.size + pathBytes.count + 1
    address.sun_len = UInt8(addressLength)
    withUnsafeMutableBytes(of: &address.sun_path) { buffer in
        buffer.initializeMemory(as: UInt8.self, repeating: 0)
        buffer.copyBytes(from: pathBytes)
    }
    let connected = withUnsafePointer(to: &address) { pointer in
        pointer.withMemoryRebound(to: sockaddr.self, capacity: 1) {
            Darwin.connect(descriptor, $0, socklen_t(addressLength))
        }
    }
    guard connected == 0 else {
        let code = errno
        Darwin.close(descriptor)
        throw NSError(
            domain: NSPOSIXErrorDomain,
            code: Int(code),
            userInfo: [NSLocalizedDescriptionKey: "连接开发语音通道失败"]
        )
    }
    return FileHandle(fileDescriptor: descriptor, closeOnDealloc: true)
}

@main
private struct SayItAppleSpeechHelper {
    static func main() async {
        let arguments = CommandLine.arguments
        if arguments.contains("--self-check") {
            Foundation.exit(await selfCheck())
        }
        guard bundleMetadata().isValid else {
            Foundation.exit(await emitInvalidIdentity())
        }
        if arguments.contains("--accumulator-check") {
            Foundation.exit(await accumulatorCheck())
        }
        if arguments.contains("--transport-check") {
            do {
                guard let socketPath = value(after: "--socket", in: arguments) else {
                    throw NSError(
                        domain: "com.henjicc.sayit.apple-speech",
                        code: 4,
                        userInfo: [NSLocalizedDescriptionKey: "缺少开发语音通道"]
                    )
                }
                let channel = try connectUnixSocket(path: socketPath)
                let emitter = JsonEmitter(output: channel)
                await emitter.send(OutputEvent(kind: "opened", backend: "TransportCheck"))
                await emitter.send(OutputEvent(kind: "finish"))
                Foundation.exit(0)
            } catch {
                let emitter = JsonEmitter()
                await emitter.send(OutputEvent(kind: "error", message: error.localizedDescription))
                Foundation.exit(1)
            }
        }
        if arguments.contains("--authorize") {
            Foundation.exit(await authorize())
        }
        let locale = value(after: "--locale", in: arguments) ?? ""
        if arguments.contains("--probe") {
            Foundation.exit(await probe(localeIdentifier: locale))
        }
        if arguments.contains("--prepare") {
            Foundation.exit(await prepare(localeIdentifier: locale))
        }
        let sampleRate = Double(value(after: "--sample-rate", in: arguments) ?? "") ?? 0
        do {
            let socketPath = value(after: "--socket", in: arguments)
            let channel = try socketPath.map(connectUnixSocket(path:))
            let input = channel ?? .standardInput
            let output = channel ?? .standardOutput
            if channel != nil {
                let emitter = JsonEmitter(output: output)
                await emitter.send(OutputEvent(
                    kind: "connected",
                    processId: ProcessInfo.processInfo.processIdentifier
                ))
            }
            Foundation.exit(await transcribe(
                localeIdentifier: locale,
                sampleRate: sampleRate,
                input: input,
                output: output
            ))
        } catch {
            let emitter = JsonEmitter()
            await emitter.send(OutputEvent(kind: "error", message: error.localizedDescription))
            Foundation.exit(1)
        }
    }

    private static func value(after key: String, in arguments: [String]) -> String? {
        guard let index = arguments.firstIndex(of: key), arguments.indices.contains(index + 1) else {
            return nil
        }
        return arguments[index + 1]
    }
}
