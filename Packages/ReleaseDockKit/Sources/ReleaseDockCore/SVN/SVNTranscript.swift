import Foundation

/// The operation an `svn` command belongs to. Set with `SVNTranscript.$scope.withValue`
/// around a pipeline operation; commands outside any scope are not recorded.
public struct TranscriptScope: Sendable, Hashable {
    public var productID: UUID
    public var releaseID: UUID?
    /// What the user asked for, e.g. "Prepare" or "Update working copy".
    public var operation: String

    public init(productID: UUID, releaseID: UUID? = nil, operation: String) {
        self.productID = productID
        self.releaseID = releaseID
        self.operation = operation
    }
}

/// One `svn` command as it ran: arguments (never a password; it goes through stdin),
/// exit code, duration and the end of its output.
public struct TranscriptEntry: Codable, Hashable, Sendable, Identifiable {
    public var id: UUID
    public var date: Date
    public var productID: UUID
    public var releaseID: UUID?
    public var operation: String
    /// `svn update --set-depth infinity ~/svn/plugin/trunk`, without the global options.
    public var command: String
    /// nil when the command could not start or was stopped (timeout, cancel).
    public var exitCode: Int32?
    public var duration: TimeInterval
    /// Last lines of stdout and stderr. XML, file contents and diffs are summarised by size.
    public var output: String
    public var failure: String?

    public var succeeded: Bool { exitCode == 0 && failure == nil }
}

public protocol SVNTranscribing: Sendable {
    func record(_ entry: TranscriptEntry)
    func entries(productID: UUID, releaseID: UUID?, limit: Int) -> [TranscriptEntry]
}

/// Appends entries to one JSON-lines file per product under `Transcripts/`, keeping the
/// newest `maximumEntries`.
public final class SVNTranscript: SVNTranscribing, @unchecked Sendable {
    @TaskLocal public static var scope: TranscriptScope?

    private let folder: URL
    private let maximumEntries: Int
    private let lock = NSLock()

    public init(folder: URL, maximumEntries: Int = 2000) {
        self.folder = folder
        self.maximumEntries = maximumEntries
    }

    public func record(_ entry: TranscriptEntry) {
        lock.withLock {
            let file = url(for: entry.productID)
            do {
                try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
                var line = try Self.encoder.encode(entry)
                line.append(0x0A)
                if let handle = try? FileHandle(forWritingTo: file) {
                    defer { try? handle.close() }
                    try handle.seekToEnd()
                    try handle.write(contentsOf: line)
                } else {
                    try line.write(to: file, options: .atomic)
                }
                trimIfNeeded(file)
            } catch {
                Log.svn.error("Could not write the SVN transcript: \(error.localizedDescription, privacy: .public)")
            }
        }
    }

    public func entries(productID: UUID, releaseID: UUID? = nil, limit: Int = 500) -> [TranscriptEntry] {
        lock.withLock {
            let all = read(url(for: productID))
            let matching = releaseID.map { id in all.filter { $0.releaseID == id } } ?? all
            return Array(matching.suffix(limit))
        }
    }

    private func url(for productID: UUID) -> URL {
        folder.appendingPathComponent("\(productID.uuidString).jsonl")
    }

    private func read(_ file: URL) -> [TranscriptEntry] {
        guard let text = try? String(contentsOf: file, encoding: .utf8) else { return [] }
        return text.split(separator: "\n").compactMap { try? Self.decoder.decode(TranscriptEntry.self, from: Data($0.utf8)) }
    }

    /// Rewrites the file with the newest entries once it is a quarter over the limit.
    private func trimIfNeeded(_ file: URL) {
        let size = (try? file.resourceValues(forKeys: [.fileSizeKey]).fileSize) ?? 0
        guard size > 256 * 1024 else { return }
        let all = read(file)
        guard all.count > maximumEntries + maximumEntries / 4 else { return }
        let kept = all.suffix(maximumEntries).compactMap { try? Self.encoder.encode($0) }
        var data = Data()
        for line in kept { data.append(line); data.append(0x0A) }
        try? data.write(to: file, options: .atomic)
    }

    private static let encoder: JSONEncoder = {
        let encoder = JSONEncoder()
        encoder.dateEncodingStrategy = .iso8601
        return encoder
    }()

    private static let decoder: JSONDecoder = {
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .iso8601
        return decoder
    }()

    // MARK: Building entries

    /// Commands whose stdout is data rather than a message: only its size is kept.
    static let dataCommands: Set<String> = ["cat", "diff", "export"]

    static func entry(scope: TranscriptScope, arguments: [String], workingDirectory: URL?, result: ProcessResult?, failure: String?, duration: TimeInterval, date: Date = Date()) -> TranscriptEntry {
        let stdout = result.map { String(decoding: $0.standardOutput, as: UTF8.self) } ?? ""
        let stderr = result.map { String(decoding: $0.standardError, as: UTF8.self) } ?? ""
        var output: [String] = []
        if !stdout.isEmpty {
            if arguments.contains("--xml") || dataCommands.contains(arguments.first ?? "") {
                output.append("(\(ByteCountFormatter.string(fromByteCount: Int64(result?.standardOutput.count ?? 0), countStyle: .file)) of output)")
            } else {
                output.append(tail(stdout, lines: 12))
            }
        }
        if !stderr.isEmpty { output.append(tail(stderr, lines: 12)) }
        return TranscriptEntry(
            id: UUID(),
            date: date,
            productID: scope.productID,
            releaseID: scope.releaseID,
            operation: scope.operation,
            command: SecretRedactor.redact((["svn"] + arguments.map(display)).joined(separator: " ")),
            exitCode: result?.exitCode,
            duration: duration,
            output: SecretRedactor.redact(output.joined(separator: "\n")),
            failure: failure.map(SecretRedactor.redact)
        )
    }

    private static func tail(_ text: String, lines: Int) -> String {
        let all = text.split(separator: "\n", omittingEmptySubsequences: false)
        let trimmed = all.last?.isEmpty == true ? all.dropLast() : all[...]
        let kept = trimmed.suffix(lines)
        let prefix = trimmed.count > lines ? "… \(trimmed.count - lines) more lines\n" : ""
        return prefix + kept.joined(separator: "\n")
    }

    /// Paths under the home folder as `~/…`; arguments with spaces quoted.
    private static func display(_ argument: String) -> String {
        var value = argument
        let home = NSHomeDirectory()
        if value.hasPrefix(home + "/") { value = "~" + value.dropFirst(home.count) }
        return value.contains(" ") ? "\"\(value)\"" : value
    }
}
