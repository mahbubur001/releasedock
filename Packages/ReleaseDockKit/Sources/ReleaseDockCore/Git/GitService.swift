import Foundation

public struct GitRepositoryState: Codable, Hashable, Sendable {
    public var isRepository: Bool
    public var headCommit: String?
    public var branch: String?
    public var exactTag: String?
    /// `git status --porcelain` lines (tracked changes and untracked files).
    public var uncommittedChanges: [String]
    public var remoteURL: String?

    public var isClean: Bool { uncommittedChanges.isEmpty }

    public static let notARepository = GitRepositoryState(isRepository: false, headCommit: nil, branch: nil, exactTag: nil, uncommittedChanges: [], remoteURL: nil)
}

public protocol GitServicing: Sendable {
    func isAvailable() async -> Bool
    /// Read-only inspection of a local development folder.
    func state(of directory: URL) async throws -> GitRepositoryState
}

/// Read-only Git inspection used to record the source commit of a release and to
/// optionally require a clean development tree. ReleaseDock never writes to Git.
public struct GitService: GitServicing {
    public static let candidatePaths = ["/usr/bin/git", "/opt/homebrew/bin/git", "/usr/local/bin/git"]

    private let runner: ProcessRunning
    private let executable: URL?

    public init(runner: ProcessRunning = ProcessRunner(), executable: URL? = nil) {
        self.runner = runner
        self.executable = executable ?? Self.candidatePaths.first(where: FileManager.default.isExecutableFile(atPath:)).map(URL.init(fileURLWithPath:))
    }

    public func isAvailable() async -> Bool {
        guard let executable else { return false }
        // /usr/bin/git is a shim that fails when the Command Line Tools are missing.
        let result = try? await runner.run(ProcessRequest(executableURL: executable, arguments: ["--version"], timeout: .seconds(20)))
        return result?.succeeded == true
    }

    private func git(_ args: [String], in directory: URL) async throws -> ProcessResult {
        guard let executable else { throw ProcessRunnerError.executableNotFound("git") }
        var env = ProcessRequest.defaultEnvironment()
        env["GIT_TERMINAL_PROMPT"] = "0"
        env["GIT_OPTIONAL_LOCKS"] = "0" // never take index.lock during read-only status
        return try await runner.run(ProcessRequest(
            executableURL: executable,
            arguments: ["-C", directory.path] + args,
            environment: env,
            timeout: .seconds(60)
        ))
    }

    public func state(of directory: URL) async throws -> GitRepositoryState {
        let inside = try await git(["rev-parse", "--is-inside-work-tree"], in: directory)
        guard inside.succeeded, inside.stdout.trimmingCharacters(in: .whitespacesAndNewlines) == "true" else {
            return .notARepository
        }
        func value(_ args: [String]) async throws -> String? {
            let result = try await git(args, in: directory)
            guard result.succeeded else { return nil }
            let text = result.stdout.trimmingCharacters(in: .whitespacesAndNewlines)
            return text.isEmpty ? nil : text
        }
        let status = try await git(["status", "--porcelain=v1", "--untracked-files=normal"], in: directory)
        return GitRepositoryState(
            isRepository: true,
            headCommit: try await value(["rev-parse", "HEAD"]),
            branch: try await value(["rev-parse", "--abbrev-ref", "HEAD"]),
            exactTag: try await value(["describe", "--tags", "--exact-match", "HEAD"]),
            uncommittedChanges: status.stdout.components(separatedBy: "\n").filter { !$0.isEmpty },
            remoteURL: try await value(["config", "--get", "remote.origin.url"]).map(SecretRedactor.redact)
        )
    }
}
