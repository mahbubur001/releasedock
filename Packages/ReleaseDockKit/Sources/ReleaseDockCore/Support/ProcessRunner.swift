import Foundation

/// A request to run an external executable. Arguments are always passed as a
/// discrete array straight to `execve` via `Process`; no shell is involved, so
/// user-controlled strings can never be interpreted as shell syntax.
public struct ProcessRequest: Sendable {
    public var executableURL: URL
    public var arguments: [String]
    public var workingDirectory: URL?
    public var environment: [String: String]
    /// Bytes written to the child's stdin, then stdin is closed. Used for
    /// `svn --password-from-stdin` so secrets never appear in `ps` output.
    public var standardInput: Data?
    public var timeout: Duration
    /// Arguments at these indices are replaced with `<redacted>` in descriptions and logs.
    public var redactedArgumentIndices: Set<Int>

    public init(
        executableURL: URL,
        arguments: [String],
        workingDirectory: URL? = nil,
        environment: [String: String] = ProcessRequest.defaultEnvironment(),
        standardInput: Data? = nil,
        timeout: Duration = .seconds(120),
        redactedArgumentIndices: Set<Int> = []
    ) {
        self.executableURL = executableURL
        self.arguments = arguments
        self.workingDirectory = workingDirectory
        self.environment = environment
        self.standardInput = standardInput
        self.timeout = timeout
        self.redactedArgumentIndices = redactedArgumentIndices
    }

    /// A minimal, predictable environment. The parent environment is not inherited
    /// so stray variables (SVN_EDITOR, GIT_DIR, proxies set by other tools…) cannot
    /// change behaviour. A UTF-8 locale is required for svn to handle non-ASCII paths.
    public static func defaultEnvironment(home: String = NSHomeDirectory()) -> [String: String] {
        [
            "PATH": "/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin:/usr/local/bin",
            "HOME": home,
            "LANG": "en_US.UTF-8",
            "LC_ALL": "en_US.UTF-8",
        ]
    }

    /// Human-readable command line with redacted arguments, for logs and diagnostics only.
    public var displayCommand: String {
        let args = arguments.enumerated().map { index, arg in
            redactedArgumentIndices.contains(index) ? "<redacted>" : arg
        }
        return ([executableURL.path] + args).joined(separator: " ")
    }
}

public struct ProcessResult: Sendable {
    public var exitCode: Int32
    public var standardOutput: Data
    public var standardError: Data
    public var duration: Duration
    public var displayCommand: String

    public init(exitCode: Int32, standardOutput: Data, standardError: Data, duration: Duration, displayCommand: String) {
        self.exitCode = exitCode
        self.standardOutput = standardOutput
        self.standardError = standardError
        self.duration = duration
        self.displayCommand = displayCommand
    }

    public var succeeded: Bool { exitCode == 0 }
    public var stdout: String { String(decoding: standardOutput, as: UTF8.self) }
    public var stderr: String { String(decoding: standardError, as: UTF8.self) }
}

public enum ProcessRunnerError: Error, Sendable, LocalizedError {
    case executableNotFound(String)
    case launchFailed(command: String, reason: String)
    /// The process exceeded its timeout and was terminated. Partial output is preserved
    /// because callers (notably SVN commit) must treat the outcome as *uncertain*.
    case timedOut(ProcessResult)
    case cancelled(ProcessResult)

    public var errorDescription: String? {
        switch self {
        case .executableNotFound(let path): "Executable not found: \(path)"
        case .launchFailed(let command, let reason): "Failed to launch \(command): \(reason)"
        case .timedOut(let result): "Timed out after \(result.duration): \(result.displayCommand)"
        case .cancelled(let result): "Cancelled: \(result.displayCommand)"
        }
    }
}

public protocol ProcessRunning: Sendable {
    func run(_ request: ProcessRequest) async throws -> ProcessResult
}

/// Runs child processes with timeout and cooperative cancellation support.
public struct ProcessRunner: ProcessRunning {
    /// Grace period between SIGTERM and SIGKILL.
    public var killGracePeriod: Duration

    public init(killGracePeriod: Duration = .seconds(5)) {
        self.killGracePeriod = killGracePeriod
    }

    public func run(_ request: ProcessRequest) async throws -> ProcessResult {
        guard FileManager.default.isExecutableFile(atPath: request.executableURL.path) else {
            throw ProcessRunnerError.executableNotFound(request.executableURL.path)
        }

        let process = Process()
        process.executableURL = request.executableURL
        process.arguments = request.arguments
        process.environment = request.environment
        if let cwd = request.workingDirectory { process.currentDirectoryURL = cwd }

        let stdoutPipe = Pipe()
        let stderrPipe = Pipe()
        process.standardOutput = stdoutPipe
        process.standardError = stderrPipe
        let stdinPipe: Pipe?
        if request.standardInput != nil {
            let pipe = Pipe()
            process.standardInput = pipe
            stdinPipe = pipe
        } else {
            process.standardInput = FileHandle.nullDevice
            stdinPipe = nil
        }

        let control = ExecutionControl(process: process, killGracePeriod: killGracePeriod)
        let clock = ContinuousClock()
        let start = clock.now

        // Termination is observed through terminationHandler: waitUntilExit() depends on a
        // run loop and can block forever on a dispatch worker thread.
        let exited = DispatchGroup()
        exited.enter()
        process.terminationHandler = { _ in exited.leave() }

        do {
            try process.run()
        } catch {
            throw ProcessRunnerError.launchFailed(command: request.displayCommand, reason: error.localizedDescription)
        }

        if let stdinPipe, let input = request.standardInput {
            let writer = stdinPipe.fileHandleForWriting
            DispatchQueue.global(qos: .userInitiated).async {
                try? writer.write(contentsOf: input)
                try? writer.close()
            }
        }

        let timeoutSeconds = Double(request.timeout.components.seconds)
            + Double(request.timeout.components.attoseconds) / 1e18
        DispatchQueue.global().asyncAfter(deadline: .now() + timeoutSeconds) {
            control.terminate(reason: .timedOut)
        }

        let (out, err) = await withTaskCancellationHandler {
            await withCheckedContinuation { (continuation: CheckedContinuation<(Data, Data), Never>) in
                DispatchQueue.global(qos: .userInitiated).async {
                    let group = DispatchGroup()
                    let outBox = DataBox()
                    let errBox = DataBox()
                    group.enter()
                    DispatchQueue.global().async {
                        outBox.data = stdoutPipe.fileHandleForReading.readDataToEndOfFile()
                        group.leave()
                    }
                    group.enter()
                    DispatchQueue.global().async {
                        errBox.data = stderrPipe.fileHandleForReading.readDataToEndOfFile()
                        group.leave()
                    }
                    exited.wait()
                    group.wait()
                    continuation.resume(returning: (outBox.data, errBox.data))
                }
            }
        } onCancel: {
            control.terminate(reason: .cancelled)
        }

        let result = ProcessResult(
            exitCode: process.terminationStatus,
            standardOutput: out,
            standardError: err,
            duration: clock.now - start,
            displayCommand: request.displayCommand
        )
        switch control.reason {
        case .timedOut: throw ProcessRunnerError.timedOut(result)
        case .cancelled: throw ProcessRunnerError.cancelled(result)
        case nil: return result
        }
    }
}

private final class DataBox: @unchecked Sendable {
    var data = Data()
}

private final class ExecutionControl: @unchecked Sendable {
    enum Reason { case timedOut, cancelled }

    private let lock = NSLock()
    private let process: Process
    private let killGracePeriod: Duration
    private var _reason: Reason?

    init(process: Process, killGracePeriod: Duration) {
        self.process = process
        self.killGracePeriod = killGracePeriod
    }

    var reason: Reason? {
        lock.withLock { _reason }
    }

    func terminate(reason: Reason) {
        let shouldTerminate: Bool = lock.withLock {
            guard _reason == nil, process.isRunning else { return false }
            _reason = reason
            return true
        }
        guard shouldTerminate else { return }
        process.terminate()
        let pid = process.processIdentifier
        let grace = Double(killGracePeriod.components.seconds)
        DispatchQueue.global().asyncAfter(deadline: .now() + grace) { [process] in
            if process.isRunning { kill(pid, SIGKILL) }
        }
    }
}
