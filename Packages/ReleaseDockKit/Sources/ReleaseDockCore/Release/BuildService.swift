import Darwin
import Foundation

/// Outcome of a product's build command, run on a copy of the development folder.
public struct BuildResult: Codable, Hashable, Sendable {
    public var command: String
    public var exitCode: Int32
    public var durationSeconds: Double
    /// The end of the combined output, for the report.
    public var outputTail: String
    /// The built copy that is inspected and released.
    public var builtPath: String

    public init(command: String, exitCode: Int32, durationSeconds: Double, outputTail: String, builtPath: String) {
        self.command = command
        self.exitCode = exitCode
        self.durationSeconds = durationSeconds
        self.outputTail = outputTail
        self.builtPath = builtPath
    }
}

public protocol Building: Sendable {
    /// Copies `source` into `workspace` and runs `command` there. Never modifies `source`.
    func build(source: URL, command: String, workspace: URL) async throws -> BuildResult
}

/// Runs a product's build command (e.g. `composer install --no-dev && npm run build`) in a
/// login `zsh`, so the user's Homebrew/Node setup from `~/.zprofile` applies. The command is
/// the user's own configuration and runs with their permissions, on a clone of the folder.
public struct BuildService: Building {
    public static let outputTailLength = 6000

    private let runner: ProcessRunning
    private let shell: URL
    private let timeout: Duration

    public init(runner: ProcessRunning = ProcessRunner(), shell: URL = URL(fileURLWithPath: "/bin/zsh"), timeout: Duration = .seconds(1800)) {
        self.runner = runner
        self.shell = shell
        self.timeout = timeout
    }

    public func build(source: URL, command: String, workspace: URL) async throws -> BuildResult {
        let target = workspace.appendingPathComponent("build", isDirectory: true).appendingPathComponent(source.lastPathComponent, isDirectory: true)
        try Self.cloneTree(from: source, to: target)

        let request = ProcessRequest(executableURL: shell, arguments: ["-l", "-c", command], workingDirectory: target, timeout: timeout)
        let result: ProcessResult
        do {
            result = try await runner.run(request)
        } catch ProcessRunnerError.timedOut(let partial) {
            throw ReleaseError.buildFailed(command: command, exitCode: partial.exitCode, output: Self.tail(partial) + "\n(timed out after \(partial.duration))")
        }
        guard result.succeeded else {
            throw ReleaseError.buildFailed(command: command, exitCode: result.exitCode, output: Self.tail(result))
        }
        let seconds = Double(result.duration.components.seconds) + Double(result.duration.components.attoseconds) / 1e18
        return BuildResult(command: command, exitCode: result.exitCode, durationSeconds: seconds, outputTail: Self.tail(result), builtPath: target.path)
    }

    static func tail(_ result: ProcessResult) -> String {
        let combined = [result.stdout, result.stderr].filter { !$0.isEmpty }.joined(separator: "\n")
        return String(combined.suffix(outputTailLength))
    }

    /// Clones the tree with APFS `clonefile` (copy-on-write, near instant), falling back to
    /// a regular copy across volumes or file systems without cloning.
    static func cloneTree(from source: URL, to target: URL) throws {
        let fm = FileManager.default
        if fm.fileExists(atPath: target.path) { try fm.removeItem(at: target) }
        try fm.createDirectory(at: target.deletingLastPathComponent(), withIntermediateDirectories: true)
        if clonefile(source.path, target.path, 0) == 0 { return }
        try fm.copyItem(at: source, to: target)
    }
}
