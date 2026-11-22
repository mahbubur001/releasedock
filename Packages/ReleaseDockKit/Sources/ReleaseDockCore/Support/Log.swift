import Foundation
import OSLog

/// Structured loggers. Values that could identify users or contain secrets must be
/// logged with `privacy: .private`; credentials are never passed to a logger at all.
public enum Log {
    public static let subsystem = "com.releasedock.app"

    public static let app = Logger(subsystem: subsystem, category: "app")
    public static let release = Logger(subsystem: subsystem, category: "release")
    public static let svn = Logger(subsystem: subsystem, category: "svn")
    public static let git = Logger(subsystem: subsystem, category: "git")
    public static let inspection = Logger(subsystem: subsystem, category: "inspection")
    public static let persistence = Logger(subsystem: subsystem, category: "persistence")
    public static let github = Logger(subsystem: subsystem, category: "github")
    public static let updates = Logger(subsystem: subsystem, category: "updates")
    public static let keychain = Logger(subsystem: subsystem, category: "keychain")
}

/// Removes anything that looks like a credential from free-form text before it is
/// persisted (release events, diagnostics). Defence in depth: secrets are never
/// intentionally placed in such text.
public enum SecretRedactor {
    private static let patterns: [NSRegularExpression] = [
        // --password VALUE / --password=VALUE
        try! NSRegularExpression(pattern: #"(--password[= ])\S+"#),
        // Authorization headers
        try! NSRegularExpression(pattern: #"(?i)(authorization:\s*(?:bearer|token|basic)\s+)\S+"#),
        // GitHub tokens
        try! NSRegularExpression(pattern: #"\b(gh[pousr]_|github_pat_)[A-Za-z0-9_]{16,}"#),
        // user:password@host in URLs
        try! NSRegularExpression(pattern: #"(://[^/\s:@]+:)[^@\s/]+(?=@)"#),
    ]

    public static func redact(_ text: String) -> String {
        var result = text
        for regex in patterns {
            let range = NSRange(result.startIndex..., in: result)
            let template = regex.numberOfCaptureGroups > 0 ? "$1<redacted>" : "<redacted>"
            result = regex.stringByReplacingMatches(in: result, range: range, withTemplate: template)
        }
        return result
    }
}
