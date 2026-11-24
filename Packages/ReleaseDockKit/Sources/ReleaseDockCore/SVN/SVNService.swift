import Foundation

public protocol SVNServicing: Sendable {
    func detectInstallation() async throws -> SVNInstallation
    func info(_ target: String) async throws -> SVNInfo
    /// True if the URL/path exists; false only when svn positively reports it missing.
    /// Network and authentication failures throw, so they are never mistaken for "missing".
    func exists(_ target: String) async throws -> Bool
    func list(_ url: String, recursive: Bool) async throws -> [SVNListEntry]
    func cat(_ url: String) async throws -> Data
    func checkout(_ url: String, to path: URL, depth: SVNDepth) async throws
    /// Updates and returns the working-copy revision.
    @discardableResult
    func update(_ path: URL, setDepth: SVNDepth?) async throws -> Int
    func status(_ path: URL, showUpdates: Bool) async throws -> [SVNStatusEntry]
    func diff(_ path: URL) async throws -> String
    func add(_ paths: [URL]) async throws
    func delete(_ paths: [URL]) async throws
    func copy(_ source: URL, to destination: URL) async throws
    func revert(_ path: URL) async throws
    func cleanup(_ path: URL) async throws
    func setProperty(_ name: String, value: String, paths: [URL]) async throws
    /// Commits and returns the new revision. Throws `SVNError.commitOutcomeUncertain` when
    /// the result cannot be known (timeout, interruption) — callers must not retry blindly.
    func commit(_ paths: [URL], message: String, credentials: SVNCredentials?) async throws -> Int
    func log(_ url: String, limit: Int) async throws -> [SVNLogEntry]
    func export(_ url: String, to path: URL) async throws
    /// Whether svn has cached credentials whose realm matches the repository host.
    func hasCachedCredentials(forRepository url: URL) async throws -> Bool
}

/// Drives the installed `svn` command-line client through `ProcessRunner`.
/// Every invocation uses `--non-interactive`, discrete arguments (no shell), and
/// `@`-escaped targets so file names like `logo@2x.png` are not read as peg revisions.
public final class SVNService: SVNServicing, @unchecked Sendable {
    public static let candidatePaths = [
        "/opt/homebrew/bin/svn",
        "/usr/local/bin/svn",
        "/opt/local/bin/svn",
        "/usr/bin/svn",
    ]

    private let runner: ProcessRunning
    private let lock = NSLock()
    private var executable: URL?
    private let preferredExecutable: URL?
    public var commandTimeout: Duration
    public var commitTimeout: Duration
    /// HTTP timeout passed to svn (seconds) for reads, so a dead connection fails fast.
    /// svn's own default is 600 s.
    public var httpTimeoutSeconds: Int
    /// HTTP timeout for commits. WordPress.org can take minutes to finish a large commit
    /// ("Committing transaction…"); a short timeout there turns a successful commit into
    /// E175012 and an uncertain outcome.
    public var commitHTTPTimeoutSeconds: Int

    public init(
        runner: ProcessRunning = ProcessRunner(),
        executable: URL? = nil,
        commandTimeout: Duration = .seconds(300),
        commitTimeout: Duration = .seconds(1800),
        httpTimeoutSeconds: Int = 120,
        commitHTTPTimeoutSeconds: Int = 1800
    ) {
        self.runner = runner
        preferredExecutable = executable
        self.commandTimeout = commandTimeout
        self.commitTimeout = commitTimeout
        self.httpTimeoutSeconds = httpTimeoutSeconds
        self.commitHTTPTimeoutSeconds = commitHTTPTimeoutSeconds
    }

    // MARK: Installation

    public func detectInstallation() async throws -> SVNInstallation {
        let fm = FileManager.default
        let candidates = (preferredExecutable.map { [$0.path] } ?? []) + Self.candidatePaths
        for path in candidates where fm.isExecutableFile(atPath: path) {
            let url = URL(fileURLWithPath: path)
            let result = try await runner.run(ProcessRequest(executableURL: url, arguments: ["--version", "--quiet"], timeout: .seconds(20)))
            guard result.succeeded else { continue }
            let version = result.stdout.trimmingCharacters(in: .whitespacesAndNewlines)
            lock.withLock { executable = url }
            return SVNInstallation(executableURL: url, version: version)
        }
        throw SVNError.notInstalled
    }

    private func svnExecutable() async throws -> URL {
        if let cached = lock.withLock({ executable }) { return cached }
        return try await detectInstallation().executableURL
    }

    // MARK: Running

    /// Appends `@` to targets containing `@`, the documented way to stop svn treating
    /// the text after the last `@` as a peg revision.
    static func target(_ raw: String) -> String {
        raw.contains("@") ? raw + "@" : raw
    }

    static func target(_ url: URL) -> String {
        target(url.isFileURL ? url.path : url.absoluteString)
    }

    static func globalOptions(httpTimeout: Int) -> [String] {
        ["--non-interactive", "--config-option", "servers:global:http-timeout=\(httpTimeout)"]
    }

    @discardableResult
    private func svn(_ arguments: [String], credentials: SVNCredentials? = nil, workingDirectory: URL? = nil, timeout: Duration? = nil, httpTimeout: Int? = nil, throwOnFailure: Bool = true) async throws -> ProcessResult {
        let executable = try await svnExecutable()
        var args = arguments + Self.globalOptions(httpTimeout: httpTimeout ?? httpTimeoutSeconds)
        var stdin: Data?
        if let credentials {
            // Password via stdin (requires --non-interactive); --no-auth-cache keeps it out of ~/.subversion.
            args += ["--username", credentials.username, "--password-from-stdin", "--no-auth-cache"]
            stdin = Data((credentials.password + "\n").utf8)
        }
        let request = ProcessRequest(
            executableURL: executable,
            arguments: args,
            workingDirectory: workingDirectory,
            standardInput: stdin,
            timeout: timeout ?? commandTimeout
        )
        let start = Date()
        let result = try await runner.run(request)
        Log.svn.debug("svn \(arguments.first ?? "", privacy: .public) exit=\(result.exitCode) in \(Date().timeIntervalSince(start), format: .fixed(precision: 2))s")
        guard result.succeeded || !throwOnFailure else { throw Self.error(from: result) }
        return result
    }

    static func errorCodes(in stderr: String) -> [String] {
        let regex = try! NSRegularExpression(pattern: #"svn: (?:warning: )?([EW]\d{6}):"#)
        return regex.matches(in: stderr, range: NSRange(stderr.startIndex..., in: stderr)).compactMap {
            Range($0.range(at: 1), in: stderr).map { String(stderr[$0]) }
        }
    }

    static func error(from result: ProcessResult) -> SVNError {
        let stderr = SecretRedactor.redact(result.stderr)
        let codes = errorCodes(in: stderr)
        let message = stderr
            .components(separatedBy: .newlines)
            .map { $0.trimmingCharacters(in: .whitespaces) }
            .filter { !$0.isEmpty }
            .suffix(4)
            .joined(separator: " ")
        return .commandFailed(kind: SVNErrorKind.classify(codes: codes, stderr: stderr), codes: codes, message: message, command: result.displayCommand)
    }

    private func xml(_ result: ProcessResult) throws -> XMLElement {
        do {
            return try XMLElement.parse(result.standardOutput)
        } catch {
            throw SVNError.unexpectedOutput("could not parse XML from \(result.displayCommand)")
        }
    }

    // MARK: Queries

    public func info(_ target: String) async throws -> SVNInfo {
        let result = try await svn(["info", "--xml", Self.target(target)])
        guard let entry = try xml(result).descendants("entry").first else {
            throw SVNError.unexpectedOutput("svn info returned no entry")
        }
        return Self.parseInfo(entry)
    }

    static func parseInfo(_ entry: XMLElement) -> SVNInfo {
        let commit = entry.child("commit")
        return SVNInfo(
            url: entry.child("url")?.trimmedText ?? "",
            repositoryRoot: entry.child("repository")?.child("root")?.trimmedText ?? "",
            repositoryUUID: entry.child("repository")?.child("uuid")?.trimmedText,
            revision: Int(entry.attributes["revision"] ?? "") ?? 0,
            kind: SVNNodeKind(rawValue: entry.attributes["kind"] ?? "") ?? .unknown,
            lastChangedRevision: commit.flatMap { Int($0.attributes["revision"] ?? "") },
            lastChangedAuthor: commit?.child("author")?.trimmedText,
            lastChangedDate: commit?.child("date")?.trimmedText,
            workingCopyRoot: entry.child("wc-info")?.child("wcroot-abspath")?.trimmedText
        )
    }

    public func exists(_ target: String) async throws -> Bool {
        do {
            _ = try await info(target)
            return true
        } catch let error as SVNError where error.kind == .notFound {
            return false
        }
    }

    public func list(_ url: String, recursive: Bool) async throws -> [SVNListEntry] {
        var args = ["list", "--xml"]
        if recursive { args.append("--recursive") }
        args.append(Self.target(url))
        let result = try await svn(args)
        return try xml(result).descendants("entry").map { entry in
            SVNListEntry(
                name: entry.child("name")?.trimmedText ?? "",
                kind: SVNNodeKind(rawValue: entry.attributes["kind"] ?? "") ?? .unknown,
                size: entry.child("size").flatMap { UInt64($0.trimmedText) },
                commitRevision: entry.child("commit").flatMap { Int($0.attributes["revision"] ?? "") }
            )
        }
    }

    public func cat(_ url: String) async throws -> Data {
        try await svn(["cat", Self.target(url)]).standardOutput
    }

    public func status(_ path: URL, showUpdates: Bool) async throws -> [SVNStatusEntry] {
        // --no-ignore: files matching svn's global-ignores (e.g. *.a, *.so, .DS_Store) would
        // otherwise be invisible and silently left out of the release.
        var args = ["status", "--xml", "--no-ignore"]
        if showUpdates { args.append("--show-updates") }
        args.append(Self.target(path))
        let result = try await svn(args)
        return try Self.parseStatus(xml(result))
    }

    static func parseStatus(_ root: XMLElement) -> [SVNStatusEntry] {
        root.descendants("entry").compactMap { entry in
            guard let path = entry.attributes["path"], let wc = entry.child("wc-status") else { return nil }
            let repos = entry.child("repos-status")
            return SVNStatusEntry(
                path: path,
                item: SVNItemStatus(rawValue: wc.attributes["item"] ?? "") ?? .none,
                props: SVNItemStatus(rawValue: wc.attributes["props"] ?? "") ?? .none,
                copied: wc.attributes["copied"] == "true",
                isTreeConflict: wc.attributes["tree-conflicted"] == "true",
                repositoryItem: repos.flatMap { SVNItemStatus(rawValue: $0.attributes["item"] ?? "") },
                repositoryProps: repos.flatMap { SVNItemStatus(rawValue: $0.attributes["props"] ?? "") },
                workingRevision: Int(wc.attributes["revision"] ?? "")
            )
        }
    }

    public func diff(_ path: URL) async throws -> String {
        // Text diff only; binary files are reported by svn as "Cannot display".
        try await svn(["diff", "--internal-diff", Self.target(path)]).stdout
    }

    public func log(_ url: String, limit: Int) async throws -> [SVNLogEntry] {
        let result = try await svn(["log", "--xml", "--verbose", "--limit", String(limit), Self.target(url)])
        return try xml(result).descendants("logentry").map { entry in
            SVNLogEntry(
                revision: Int(entry.attributes["revision"] ?? "") ?? 0,
                author: entry.child("author")?.trimmedText,
                date: entry.child("date")?.trimmedText,
                message: entry.child("msg")?.text ?? "",
                changedPaths: entry.child("paths")?.all("path").map(\.trimmedText) ?? []
            )
        }
    }

    public func hasCachedCredentials(forRepository url: URL) async throws -> Bool {
        guard let host = url.host else { return false }
        let pattern = "<\(url.scheme ?? "https")://\(host):*"
        let result = try await svn(["auth", pattern])
        // `svn auth` prints "Credentials cache in '…' contains N credentials" and one block per match.
        return result.stdout.contains("Username:")
    }

    // MARK: Working copy

    public func checkout(_ url: String, to path: URL, depth: SVNDepth) async throws {
        try await svn(["checkout", "--depth", depth.rawValue, Self.target(url), Self.target(path)])
    }

    @discardableResult
    public func update(_ path: URL, setDepth: SVNDepth?) async throws -> Int {
        var args = ["update"]
        if let setDepth { args += ["--set-depth", setDepth.rawValue] }
        args.append(Self.target(path))
        let result = try await svn(args)
        let regex = try! NSRegularExpression(pattern: #"(?:At|Updated to) revision (\d+)\."#)
        let out = result.stdout
        if let match = regex.matches(in: out, range: NSRange(out.startIndex..., in: out)).last,
           let range = Range(match.range(at: 1), in: out), let revision = Int(out[range]) {
            return revision
        }
        return try await info(path.path).revision
    }

    public func add(_ paths: [URL]) async throws {
        try await withTargets(paths) { targets in
            // --parents adds intermediate directories; --no-ignore so global-ignores never drop files.
            try await self.svn(["add", "--parents", "--no-ignore", "--force", "--depth", "infinity", "--targets", targets.path])
        }
    }

    public func delete(_ paths: [URL]) async throws {
        try await withTargets(paths) { targets in
            try await self.svn(["delete", "--force", "--targets", targets.path])
        }
    }

    public func copy(_ source: URL, to destination: URL) async throws {
        try await svn(["copy", "--parents", Self.target(source), Self.target(destination)])
    }

    public func revert(_ path: URL) async throws {
        try await svn(["revert", "--recursive", Self.target(path)])
    }

    public func cleanup(_ path: URL) async throws {
        try await svn(["cleanup", Self.target(path)])
    }

    public func setProperty(_ name: String, value: String, paths: [URL]) async throws {
        try await withTargets(paths) { targets in
            try await self.svn(["propset", name, value, "--targets", targets.path])
        }
    }

    public func commit(_ paths: [URL], message: String, credentials: SVNCredentials?) async throws -> Int {
        // The message goes into a file: stdin may be carrying the password.
        let messageFile = FileManager.default.temporaryDirectory.appendingPathComponent("releasedock-commit-\(UUID().uuidString).txt")
        try Data(message.utf8).write(to: messageFile, options: [.atomic])
        defer { try? FileManager.default.removeItem(at: messageFile) }

        let args = ["commit", "--file", messageFile.path, "--encoding", "UTF-8"] + paths.map(Self.target)
        let result: ProcessResult
        do {
            result = try await svn(args, credentials: credentials, timeout: commitTimeout, httpTimeout: commitHTTPTimeoutSeconds, throwOnFailure: false)
        } catch ProcessRunnerError.timedOut(let partial) {
            if let revision = Self.committedRevision(in: partial.stdout) { return revision }
            throw SVNError.commitOutcomeUncertain("timed out after \(partial.duration)")
        } catch ProcessRunnerError.cancelled(let partial) {
            if let revision = Self.committedRevision(in: partial.stdout) { return revision }
            throw SVNError.commitOutcomeUncertain("interrupted")
        }
        // "Committed revision N." is printed only after the server accepted the commit,
        // even if a post-commit step then reports an error.
        if let revision = Self.committedRevision(in: result.stdout) { return revision }
        if result.succeeded {
            if result.stdout.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                throw SVNError.unexpectedOutput("svn commit reported nothing to commit")
            }
            throw SVNError.commitOutcomeUncertain("could not read the committed revision")
        }
        let error = Self.error(from: result)
        switch error.kind {
        // Rejected before the transaction was committed: definitely not in the repository.
        case .authenticationRequired?, .authenticationFailed?, .forbidden?, .hookRejected?, .outOfDate?,
             .conflict?, .alreadyExists?, .workingCopyLocked?, .notAWorkingCopy?, .certificate?, .notFound?:
            throw error
        // Network failures and anything unrecognised may have happened after the server
        // committed; never assume either way.
        default:
            throw SVNError.commitOutcomeUncertain(error.localizedDescription)
        }
    }

    static func committedRevision(in stdout: String) -> Int? {
        let regex = try! NSRegularExpression(pattern: #"Committed revision (\d+)\."#)
        guard let match = regex.firstMatch(in: stdout, range: NSRange(stdout.startIndex..., in: stdout)),
              let range = Range(match.range(at: 1), in: stdout) else { return nil }
        return Int(stdout[range])
    }

    public func export(_ url: String, to path: URL) async throws {
        try await svn(["export", "--force", Self.target(url), Self.target(path)])
    }

    /// Passes long path lists through a `--targets` file to avoid argv limits.
    private func withTargets(_ paths: [URL], _ body: (URL) async throws -> Void) async throws {
        guard !paths.isEmpty else { return }
        let file = FileManager.default.temporaryDirectory.appendingPathComponent("releasedock-targets-\(UUID().uuidString).txt")
        let content = paths.map { Self.target($0) }.joined(separator: "\n") + "\n"
        try Data(content.utf8).write(to: file, options: [.atomic])
        defer { try? FileManager.default.removeItem(at: file) }
        try await body(file)
    }
}
