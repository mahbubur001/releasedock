import Foundation

public enum CodeCheckTool: String, Codable, Sendable {
    case phpLint
    case pluginCheck
}

public struct CodeCheckFinding: Codable, Hashable, Sendable {
    public var tool: CodeCheckTool
    public var severity: ValidationSeverity
    public var file: String?
    public var line: Int?
    public var code: String?
    public var message: String

    public init(tool: CodeCheckTool, severity: ValidationSeverity, file: String?, line: Int?, code: String? = nil, message: String) {
        self.tool = tool
        self.severity = severity
        self.file = file
        self.line = line
        self.code = code
        self.message = message
    }
}

/// Outcome of the optional code checks run on the staged release candidate.
public struct CodeCheckReport: Codable, Hashable, Sendable {
    public var phpVersion: String?
    public var lintedFiles: Int
    /// Why PHP lint did not run (no php, local PHP older than Requires PHP…); nil when it ran.
    public var phpLintSkipped: String?
    public var pluginCheckRan: Bool
    /// Why Plugin Check did not run; nil when it ran.
    public var pluginCheckSkipped: String?
    public var findings: [CodeCheckFinding]

    public init(phpVersion: String? = nil, lintedFiles: Int = 0, phpLintSkipped: String? = nil, pluginCheckRan: Bool = false, pluginCheckSkipped: String? = nil, findings: [CodeCheckFinding] = []) {
        self.phpVersion = phpVersion
        self.lintedFiles = lintedFiles
        self.phpLintSkipped = phpLintSkipped
        self.pluginCheckRan = pluginCheckRan
        self.pluginCheckSkipped = pluginCheckSkipped
        self.findings = findings
    }

    public func findings(_ tool: CodeCheckTool) -> [CodeCheckFinding] { findings.filter { $0.tool == tool } }
}

public struct CodeCheckConfiguration: Sendable, Equatable {
    /// Root of a local WordPress install with the Plugin Check plugin active. Nil or empty
    /// disables Plugin Check.
    public var pluginCheckWordPressPath: String?

    public init(pluginCheckWordPressPath: String? = nil) {
        self.pluginCheckWordPressPath = pluginCheckWordPressPath
    }
}

/// Thread-safe holder so the app can change the configuration of a long-lived service.
public final class CodeCheckConfigurationStore: @unchecked Sendable {
    private let lock = NSLock()
    private var _value: CodeCheckConfiguration

    public init(_ value: CodeCheckConfiguration = CodeCheckConfiguration()) { _value = value }

    public var value: CodeCheckConfiguration {
        get { lock.withLock { _value } }
        set { lock.withLock { _value = newValue } }
    }
}

public protocol CodeChecking: Sendable {
    /// Checks the candidate at `root`. `phpFiles` are root-relative paths. Never throws:
    /// a tool that cannot run is reported as skipped.
    func check(root: URL, phpFiles: [String], product: Product, requiresPHP: String?) async -> CodeCheckReport
}

/// PHP syntax lint (`php -l`) and, when configured, WordPress.org's Plugin Check
/// (`wp plugin check`). Both only read the candidate.
public struct CodeCheckService: CodeChecking {
    public static let phpCandidates = ["/opt/homebrew/bin/php", "/usr/local/bin/php", "/usr/bin/php"]
    public static let wpCandidates = ["/opt/homebrew/bin/wp", "/usr/local/bin/wp"]

    private let runner: ProcessRunning
    private let php: URL?
    private let wp: URL?
    private let configuration: CodeCheckConfigurationStore

    public init(runner: ProcessRunning = ProcessRunner(), php: URL? = nil, wp: URL? = nil, configuration: CodeCheckConfigurationStore = CodeCheckConfigurationStore()) {
        self.runner = runner
        let fm = FileManager.default
        self.php = php ?? Self.phpCandidates.first(where: fm.isExecutableFile(atPath:)).map(URL.init(fileURLWithPath:))
        self.wp = wp ?? Self.wpCandidates.first(where: fm.isExecutableFile(atPath:)).map(URL.init(fileURLWithPath:))
        self.configuration = configuration
    }

    public func check(root: URL, phpFiles: [String], product: Product, requiresPHP: String?) async -> CodeCheckReport {
        var report = CodeCheckReport()
        await lint(root: root, files: phpFiles, requiresPHP: requiresPHP, into: &report)
        await pluginCheck(root: root, product: product, into: &report)
        return report
    }

    // MARK: PHP lint

    private func lint(root: URL, files: [String], requiresPHP: String?, into report: inout CodeCheckReport) async {
        guard let php else {
            report.phpLintSkipped = "PHP is not installed (brew install php)"
            return
        }
        guard let version = try? await runner.run(ProcessRequest(executableURL: php, arguments: ["-n", "-r", "echo PHP_VERSION;"], timeout: .seconds(20))),
              version.succeeded, !version.stdout.isEmpty
        else {
            report.phpLintSkipped = "php at \(php.path) did not run"
            return
        }
        let phpVersion = version.stdout.trimmingCharacters(in: .whitespacesAndNewlines)
        report.phpVersion = phpVersion
        if let requiresPHP, PHPVersion.compare(phpVersion, requiresPHP) < 0 {
            report.phpLintSkipped = "Local PHP \(phpVersion) is older than Requires PHP \(requiresPHP); newer syntax would be reported as errors"
            return
        }
        guard !files.isEmpty else { return }
        // PHP 8.3+ lints several files per invocation.
        let batchSize = PHPVersion.compare(phpVersion, "8.3") >= 0 ? 100 : 1
        var start = 0
        while start < files.count {
            let batch = Array(files[start..<min(start + batchSize, files.count)])
            start += batchSize
            do {
                let result = try await runner.run(ProcessRequest(
                    executableURL: php,
                    arguments: ["-n", "-d", "display_errors=1", "-d", "log_errors=0", "-d", "error_reporting=-1", "-l"] + batch,
                    workingDirectory: root,
                    timeout: .seconds(120)
                ))
                report.findings += Self.parseLint(result.stdout + "\n" + result.stderr)
                report.lintedFiles += batch.count
            } catch {
                report.phpLintSkipped = "php -l failed: \(error.localizedDescription)"
                return
            }
        }
    }

    /// `Parse error: syntax error, … in includes/a.php on line 3`. Messages can contain " in "
    /// themselves, so the file is taken from the last one.
    static func parseLint(_ output: String) -> [CodeCheckFinding] {
        let pattern = #"(?m)^(?:PHP )?(Parse error|Fatal error|Deprecated|Warning):\s*(.+) in (.+?) on line (\d+)\s*$"#
        guard let regex = try? NSRegularExpression(pattern: pattern) else { return [] }
        var seen: Set<String> = []
        var findings: [CodeCheckFinding] = []
        for match in regex.matches(in: output, range: NSRange(output.startIndex..., in: output)) {
            func group(_ i: Int) -> String { Range(match.range(at: i), in: output).map { String(output[$0]) } ?? "" }
            let kind = group(1)
            let key = "\(group(3)):\(group(4)):\(group(2))"
            guard seen.insert(key).inserted else { continue }
            findings.append(CodeCheckFinding(
                tool: .phpLint,
                severity: kind.hasSuffix("error") ? .error : .warning,
                file: group(3),
                line: Int(group(4)),
                message: "\(kind): \(group(2))"
            ))
        }
        return findings
    }

    // MARK: Plugin Check

    private func pluginCheck(root: URL, product: Product, into report: inout CodeCheckReport) async {
        guard product.type == .plugin else { return }
        guard let path = configuration.value.pluginCheckWordPressPath?.trimmingCharacters(in: .whitespaces), !path.isEmpty else {
            report.pluginCheckSkipped = "Not set up: choose a local WordPress install with Plugin Check in Settings"
            return
        }
        guard let wp else {
            report.pluginCheckSkipped = "WP-CLI is not installed (brew install wp-cli)"
            return
        }
        do {
            let result = try await runner.run(ProcessRequest(
                executableURL: wp,
                arguments: [
                    "--path=\(path)", "plugin", "check", root.path,
                    "--slug=\(product.slug)",
                    "--mode=\(product.currentVersion == nil ? "new" : "update")",
                    "--format=strict-json",
                    "--fields=file,line,type,code,message",
                ],
                timeout: .seconds(300)
            ))
            if let findings = Self.parsePluginCheck(result.stdout) {
                report.findings += findings
                report.pluginCheckRan = true
            } else if result.succeeded {
                report.pluginCheckRan = true // nothing found
            } else {
                let reason = result.stderr.split(separator: "\n").first.map(String.init) ?? "exit code \(result.exitCode)"
                report.pluginCheckSkipped = "wp plugin check failed: \(reason)"
            }
        } catch {
            report.pluginCheckSkipped = "wp plugin check failed: \(error.localizedDescription)"
        }
    }

    /// Decodes the JSON array printed by `--format=strict-json`. WP-CLI and PHP may print
    /// notices around it, so each line that starts with `[` is tried. Nil when there is none.
    static func parsePluginCheck(_ output: String) -> [CodeCheckFinding]? {
        struct Item: Decodable {
            var file: String?
            var line: Int?
            var type: String?
            var code: String?
            var message: String?
        }
        guard let end = output.lastIndex(of: "]") else { return nil }
        var items: [Item]?
        var lineStart = output.startIndex
        while items == nil, lineStart < end {
            let trimmed = output[lineStart...].drop { $0 == " " || $0 == "\t" }
            if trimmed.first == "[" {
                items = try? JSONDecoder().decode([Item].self, from: Data(output[trimmed.startIndex...end].utf8))
            }
            guard let newline = output[lineStart...].firstIndex(of: "\n") else { break }
            lineStart = output.index(after: newline)
        }
        guard let items else { return nil }
        return items.map { item in
            CodeCheckFinding(
                tool: .pluginCheck,
                severity: item.type?.uppercased() == "ERROR" ? .error : .warning,
                file: item.file,
                line: item.line,
                code: item.code,
                message: item.message ?? item.code ?? "Plugin Check finding"
            )
        }
    }
}
