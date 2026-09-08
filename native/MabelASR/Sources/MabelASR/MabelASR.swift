import Foundation
@preconcurrency import FluidAudio
@preconcurrency import WhisperKit

/// In-process C ABI for Mabel's MAS-friendly local engines.
///
/// FluidAudio Parakeet and WhisperKit run CoreML on the Apple Neural Engine.
/// Neither path needs `disable-library-validation` or
/// `allow-unsigned-executable-memory`.
///
/// Pinned to FluidAudio 0.15.6 and WhisperKit 1.1.0. See Package.swift.
/// Swift 6 (macosx14.0): C callback is a Swift typealias, last-error is a
/// locked Sendable box, AsrManager.isAvailable is awaited, WhisperKit text
/// is treated as a non-optional String.

/// C ABI progress callback. Kept in Swift so Package.swift can be a pure
/// Swift target (the C header is not visible as a mixed-language module).
public typealias mabel_asr_progress_cb = @convention(c) (Double, UnsafeMutableRawPointer?) -> Void

private final class LastErrorBox: @unchecked Sendable {
    private let lock = NSLock()
    private var ptr: UnsafeMutablePointer<CChar>?

    func set(_ message: String) {
        lock.lock()
        defer { lock.unlock() }
        if let old = ptr { free(old) }
        ptr = strdup(message)
        fputs("[MabelASR] \(message)\n", stderr)
    }

    func clear() {
        lock.lock()
        defer { lock.unlock() }
        if let old = ptr {
            free(old)
            ptr = nil
        }
    }

    func peek() -> UnsafePointer<CChar>? {
        lock.lock()
        defer { lock.unlock() }
        return UnsafePointer(ptr)
    }
}

private let lastError = LastErrorBox()

private func setError(_ message: String) {
    lastError.set(message)
}

private func clearError() {
    lastError.clear()
}

private final class ProgressSink: @unchecked Sendable {
    let cb: mabel_asr_progress_cb?
    let user: UnsafeMutableRawPointer?

    init(_ cb: mabel_asr_progress_cb?, _ user: UnsafeMutableRawPointer?) {
        self.cb = cb
        self.user = user
    }

    func report(_ fraction: Double) {
        guard let cb else { return }
        let pct = min(100.0, max(0.0, fraction * 100.0))
        cb(pct, user)
    }
}

private func runBlocking<T>(_ body: @escaping @Sendable () async throws -> T) throws -> T {
    let semaphore = DispatchSemaphore(value: 0)
    let box = BlockingBox<T>()
    Task.detached {
        do {
            let value = try await body()
            box.set(.success(value))
        } catch {
            box.set(.failure(error))
        }
        semaphore.signal()
    }
    semaphore.wait()
    return try box.take()
}

private final class BlockingBox<T>: @unchecked Sendable {
    private var result: Result<T, Error>?
    func set(_ value: Result<T, Error>) { result = value }
    func take() throws -> T {
        guard let result else {
            throw ASRBridgeError.failed("async bridge produced no result")
        }
        return try result.get()
    }
}

private enum ASRBridgeError: Error, LocalizedError {
    case badArgument(String)
    case failed(String)
    var errorDescription: String? {
        switch self {
        case .badArgument(let s), .failed(let s): return s
        }
    }
}

private func parakeetVersion(from cString: UnsafePointer<CChar>?) -> AsrModelVersion {
    let raw = cString.flatMap { String(cString: $0) } ?? "v3"
    switch raw {
    case "v2", "en": return .v2
    default: return .v3
    }
}

private func parakeetVersionTag(_ version: AsrModelVersion) -> String {
    switch version {
    case .v2: return "v2"
    default: return "v3"
    }
}

/// FluidAudio 0.15.6 `transcribe(_:decoderState:language:)` takes `Language?`,
/// not `String?`. Mabel's C ABI still sends "en" / "multi" / nil.
private func parakeetLanguage(from raw: String?) -> Language? {
    guard let raw else { return nil }
    let code = raw.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
    switch code {
    case "", "multi", "auto":
        return nil
    default:
        return Language(rawValue: code)
    }
}

private func whisperKitModelName() -> String { "large-v3-turbo" }

private func whisperKitFolder(cacheDir: String) -> URL {
    URL(fileURLWithPath: cacheDir, isDirectory: true)
        .appendingPathComponent("whisperkit-large-v3-turbo", isDirectory: true)
}

private func whisperKitLooksReady(at folder: URL) -> Bool {
    let fm = FileManager.default
    guard let items = try? fm.contentsOfDirectory(atPath: folder.path), !items.isEmpty else {
        return false
    }
    return items.contains { $0.hasSuffix(".mlmodelc") || $0.hasSuffix(".mlpackage") }
        || fm.fileExists(atPath: folder.appendingPathComponent(".mabel-ready").path)
}

private func transcribedText(_ raw: String?) -> String {
    raw?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
}

/// Process-wide Parakeet session. FluidAudio's CoreML / `sharedMLArrayCache`
/// is global: constructing a new `AsrManager` and calling `loadModels` on
/// every take leaves the ANE session in a finished/empty state. Take 1
/// works; take 2+ returns no text until the process quits. Keep the manager
/// warm and only mint a fresh `TdtDecoderState` per utterance.
private actor ParakeetWarmSession {
    private var manager: AsrManager?
    private var loadedTag: String = ""
    private var decoderLayers: Int = 0

    func transcribe(
        version: AsrModelVersion,
        wavPath: String,
        languageHint: String?
    ) async throws -> String {
        let tag = parakeetVersionTag(version)
        if manager == nil || loadedTag != tag {
            let models = try await AsrModels.downloadAndLoad(version: version)
            let mgr = AsrManager(config: .default)
            try await mgr.loadModels(models)
            if !(await mgr.isAvailable) {
                throw ASRBridgeError.failed("Parakeet models are not available")
            }
            manager = mgr
            loadedTag = tag
            decoderLayers = models.version.decoderLayers
        }
        guard let manager else {
            throw ASRBridgeError.failed("Parakeet session is not available")
        }
        var state = try TdtDecoderState(decoderLayers: decoderLayers)
        let result = try await manager.transcribe(
            URL(fileURLWithPath: wavPath),
            decoderState: &state,
            language: parakeetLanguage(from: languageHint)
        )
        return transcribedText(result.text as String?)
    }
}

private let parakeetWarm = ParakeetWarmSession()

/// Same class of sticky state for WhisperKit: `WhisperKit(config)` with
/// `load: true` every take rebinds CoreML models that are already warm.
private final class WhisperKitWarm: @unchecked Sendable {
    private let lock = NSLock()
    private var kit: WhisperKit?
    private var folderPath: String?

    func load(folder: URL) async throws -> WhisperKit {
        lock.lock()
        if let kit, folderPath == folder.path {
            let existing = kit
            lock.unlock()
            return existing
        }
        lock.unlock()
        let config = WhisperKitConfig(
            model: whisperKitModelName(),
            downloadBase: folder,
            load: true
        )
        let created = try await WhisperKit(config)
        lock.lock()
        kit = created
        folderPath = folder.path
        lock.unlock()
        return created
    }
}

private let whisperKitWarm = WhisperKitWarm()

// MARK: - Parakeet

@_cdecl("mabel_asr_parakeet_ready")
public func mabel_asr_parakeet_ready(_ versionC: UnsafePointer<CChar>?) -> Int32 {
    let version = parakeetVersion(from: versionC)
    let cache = AsrModels.defaultCacheDirectory(for: version)
    return AsrModels.modelsExist(at: cache, version: version) ? 1 : 0
}

@_cdecl("mabel_asr_parakeet_download")
public func mabel_asr_parakeet_download(
    _ versionC: UnsafePointer<CChar>?,
    _ cb: mabel_asr_progress_cb?,
    _ user: UnsafeMutableRawPointer?
) -> Int32 {
    let version = parakeetVersion(from: versionC)
    let sink = ProgressSink(cb, user)
    do {
        try runBlocking {
            _ = try await AsrModels.download(
                version: version,
                progressHandler: { progress in
                    sink.report(progress.fractionCompleted)
                }
            )
        }
        sink.report(1.0)
        clearError()
        return 0
    } catch {
        setError("Parakeet download failed: \(error.localizedDescription)")
        return 1
    }
}

@_cdecl("mabel_asr_parakeet_transcribe")
public func mabel_asr_parakeet_transcribe(
    _ versionC: UnsafePointer<CChar>?,
    _ wavPathC: UnsafePointer<CChar>?,
    _ languageC: UnsafePointer<CChar>?,
    _ outText: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?
) -> Int32 {
    guard let wavPathC else {
        setError("Parakeet transcribe: missing wav path")
        return 1
    }
    let wavPath = String(cString: wavPathC)
    let version = parakeetVersion(from: versionC)
    let languageHint = languageC.flatMap { String(cString: $0) }
    do {
        let text = try runBlocking {
            // Reuse the warm AsrManager. Soft-reset is a fresh TdtDecoderState
            // inside ParakeetWarmSession — do not cleanup() or loadModels again.
            try await parakeetWarm.transcribe(
                version: version,
                wavPath: wavPath,
                languageHint: languageHint
            )
        }
        if let outText {
            outText.pointee = strdup(text)
        }
        clearError()
        return 0
    } catch {
        setError("Parakeet transcribe failed: \(error.localizedDescription)")
        return 1
    }
}

// MARK: - WhisperKit large-v3-turbo

@_cdecl("mabel_asr_whisperkit_ready")
public func mabel_asr_whisperkit_ready(_ cacheDirC: UnsafePointer<CChar>?) -> Int32 {
    guard let cacheDirC else { return 0 }
    let folder = whisperKitFolder(cacheDir: String(cString: cacheDirC))
    return whisperKitLooksReady(at: folder) ? 1 : 0
}

@_cdecl("mabel_asr_whisperkit_download")
public func mabel_asr_whisperkit_download(
    _ cacheDirC: UnsafePointer<CChar>?,
    _ cb: mabel_asr_progress_cb?,
    _ user: UnsafeMutableRawPointer?
) -> Int32 {
    guard let cacheDirC else {
        setError("WhisperKit download: missing cache dir")
        return 1
    }
    let cacheDir = String(cString: cacheDirC)
    let folder = whisperKitFolder(cacheDir: cacheDir)
    let sink = ProgressSink(cb, user)
    do {
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        sink.report(0.05)
        try runBlocking {
            let config = WhisperKitConfig(
                model: whisperKitModelName(),
                downloadBase: folder,
                load: true
            )
            _ = try await WhisperKit(config)
        }
        FileManager.default.createFile(
            atPath: folder.appendingPathComponent(".mabel-ready").path,
            contents: Data("large-v3-turbo\n".utf8)
        )
        sink.report(1.0)
        clearError()
        return 0
    } catch {
        setError("WhisperKit download failed: \(error.localizedDescription)")
        return 1
    }
}

@_cdecl("mabel_asr_whisperkit_transcribe")
public func mabel_asr_whisperkit_transcribe(
    _ cacheDirC: UnsafePointer<CChar>?,
    _ wavPathC: UnsafePointer<CChar>?,
    _ languageC: UnsafePointer<CChar>?,
    _ outText: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?
) -> Int32 {
    guard let cacheDirC, let wavPathC else {
        setError("WhisperKit transcribe: missing path")
        return 1
    }
    let folder = whisperKitFolder(cacheDir: String(cString: cacheDirC))
    let wavPath = String(cString: wavPathC)
    let language = languageC.flatMap { String(cString: $0) } ?? "en"
    do {
        let text = try runBlocking {
            let kit = try await whisperKitWarm.load(folder: folder)
            let detect = language != "en"
            // Soft-reset decode context on every take. The kit stays warm
            // (do not reconstruct WhisperKit / cleanup CoreML). Prefill
            // cache + leftover promptTokens are the long-session leak that
            // injects invented salad after ~15 minutes of toggle use.
            var options = DecodingOptions(
                task: .transcribe,
                language: detect ? nil : "en",
                detectLanguage: detect
            )
            options.usePrefillCache = false
            options.promptTokens = nil
            options.prefixTokens = nil
            // Annotate [TranscriptionResult] so Swift 6 does not bind the
            // deprecated optional-single-result overload. `.text` is String.
            let results: [TranscriptionResult] = try await kit.transcribe(
                audioPath: wavPath,
                decodeOptions: options
            )
            return results
                .map { transcribedText($0.text as String?) }
                .filter { !$0.isEmpty }
                .joined(separator: " ")
        }
        if let outText {
            outText.pointee = strdup(text)
        }
        clearError()
        return 0
    } catch {
        setError("WhisperKit transcribe failed: \(error.localizedDescription)")
        return 1
    }
}

@_cdecl("mabel_asr_string_free")
public func mabel_asr_string_free(_ s: UnsafeMutablePointer<CChar>?) {
    if let s { free(s) }
}

@_cdecl("mabel_asr_last_error")
public func mabel_asr_last_error() -> UnsafePointer<CChar>? {
    lastError.peek()
}
