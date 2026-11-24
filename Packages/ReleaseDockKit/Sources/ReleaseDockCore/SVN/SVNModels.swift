import Foundation

public struct SVNInstallation: Hashable, Sendable {
    public var executableURL: URL
    public var version: String

    public init(executableURL: URL, version: String) {
        self.executableURL = executableURL
        self.version = version
    }

    /// `--password-from-stdin` exists since Subversion 1.10.
    public var supportsPasswordFromStdin: Bool {
        PHPVersion.compare(version, "1.10.0") >= 0
    }
}

/// Credentials supplied from the Keychain. Never logged, never persisted outside the Keychain,
/// and passed to svn only through stdin.
public struct SVNCredentials: Sendable {
    public var username: String
    public var password: String

    public init(username: String, password: String) {
        self.username = username
        self.password = password
    }
}

public enum SVNNodeKind: String, Codable, Sendable {
    case file
    case dir
    case none
    case unknown
}

public struct SVNInfo: Hashable, Sendable {
    public var url: String
    public var repositoryRoot: String
    public var repositoryUUID: String?
    public var revision: Int
    public var kind: SVNNodeKind
    public var lastChangedRevision: Int?
    public var lastChangedAuthor: String?
    public var lastChangedDate: String?
    public var workingCopyRoot: String?
}

public enum SVNItemStatus: String, Codable, Sendable {
    case added, conflicted, deleted, external, ignored, incomplete, merged, missing
    case modified, none, normal, obstructed, replaced, unversioned
}

public struct SVNStatusEntry: Hashable, Sendable {
    /// Path as reported by svn (absolute when an absolute target was given).
    public var path: String
    public var item: SVNItemStatus
    public var props: SVNItemStatus
    public var copied: Bool
    public var isTreeConflict: Bool
    /// Present with `--show-updates`: the item changed on the server.
    public var repositoryItem: SVNItemStatus?
    public var repositoryProps: SVNItemStatus?
    public var workingRevision: Int?

    public var isOutOfDate: Bool {
        (repositoryItem.map { $0 != .none && $0 != .normal } ?? false)
            || (repositoryProps.map { $0 != .none && $0 != .normal } ?? false)
    }

    /// A local modification of any kind (excluding unversioned files, which callers handle).
    public var isLocalChange: Bool {
        switch item {
        case .added, .conflicted, .deleted, .missing, .modified, .replaced, .obstructed, .incomplete, .merged: true
        default: props == .modified || props == .conflicted || isTreeConflict
        }
    }

    /// A text, property or tree conflict left by an update.
    public var isConflict: Bool {
        item == .conflicted || props == .conflicted || isTreeConflict
    }
}

/// `svn resolve --accept` choices ReleaseDock uses.
public enum SVNConflictChoice: String, Sendable {
    /// Replace with the local version, dropping the server's change.
    case mineFull = "mine-full"
    /// Replace with the server version, dropping the local change.
    case theirsFull = "theirs-full"
    /// Keep the file on disk as it is (the only choice valid for tree conflicts).
    case working
}

public struct SVNLogEntry: Hashable, Sendable {
    public var revision: Int
    public var author: String?
    public var date: String?
    public var message: String
    public var changedPaths: [String]
}

public struct SVNListEntry: Hashable, Sendable {
    /// Path relative to the listed URL.
    public var name: String
    public var kind: SVNNodeKind
    public var size: UInt64?
    public var commitRevision: Int?
}

public enum SVNDepth: String, Sendable {
    case empty, files, immediates, infinity
}

public enum SVNErrorKind: String, Codable, Sendable {
    case authenticationRequired
    case authenticationFailed
    case forbidden
    case network
    case notFound
    case outOfDate
    case workingCopyLocked
    case notAWorkingCopy
    case conflict
    case alreadyExists
    /// A server-side pre-commit hook rejected the commit (WordPress.org uses these to enforce rules).
    case hookRejected
    case certificate
    case other
}

public enum SVNError: Error, LocalizedError, Sendable {
    case notInstalled
    case commandFailed(kind: SVNErrorKind, codes: [String], message: String, command: String)
    /// The commit process timed out or was interrupted; the commit may or may not exist on the server.
    case commitOutcomeUncertain(String)
    case unexpectedOutput(String)

    public var kind: SVNErrorKind? {
        if case .commandFailed(let kind, _, _, _) = self { return kind }
        return nil
    }

    public var errorDescription: String? {
        switch self {
        case .notInstalled:
            "Subversion (svn) is not installed. Install it with Homebrew: brew install subversion"
        case .commandFailed(let kind, _, let message, _):
            "\(kind.userDescription): \(message)"
        case .commitOutcomeUncertain(let detail):
            "The commit did not finish normally (\(detail)). Its outcome is unknown; ReleaseDock will inspect the repository before doing anything else."
        case .unexpectedOutput(let detail):
            "Unexpected svn output: \(detail)"
        }
    }
}

extension SVNErrorKind {
    var userDescription: String {
        switch self {
        case .authenticationRequired: "SVN credentials are required"
        case .authenticationFailed: "SVN authentication failed"
        case .forbidden: "Access forbidden"
        case .network: "Network error"
        case .notFound: "Not found"
        case .outOfDate: "Working copy is out of date"
        case .workingCopyLocked: "Working copy is locked"
        case .notAWorkingCopy: "Not an SVN working copy"
        case .conflict: "Conflict"
        case .alreadyExists: "Already exists"
        case .hookRejected: "The repository rejected the commit"
        case .certificate: "Server certificate could not be verified"
        case .other: "SVN error"
        }
    }

    /// Classifies svn stderr by its `E`/`W` error codes (stable across locales).
    static func classify(codes: [String], stderr: String) -> SVNErrorKind {
        let set = Set(codes)
        let lower = stderr.lowercased()
        if set.contains("E215004") || lower.contains("no more credentials") { return .authenticationRequired }
        if set.contains("E170001") || lower.contains("authorization failed") { return .authenticationFailed }
        if set.contains("E175013") || lower.contains("403 forbidden") { return .forbidden }
        if set.contains("E165001") || lower.contains("commit blocked by pre-commit hook") { return .hookRejected }
        if set.contains("E230001") || lower.contains("server certificate verification failed") { return .certificate }
        if !set.isDisjoint(with: ["E155011", "E160024", "E160028", "E170004", "E155036"]) || lower.contains("out of date") || lower.contains("out-of-date") { return .outOfDate }
        if set.contains("E155004") || set.contains("E155037") { return .workingCopyLocked }
        if set.contains("E155007") { return .notAWorkingCopy }
        if set.contains("E155015") || set.contains("E155035") || lower.contains("conflict") { return .conflict }
        if set.contains("E160020") || set.contains("E150002") || lower.contains("already exists") { return .alreadyExists }
        if !set.isDisjoint(with: ["E170000", "W170000", "E160013", "W160013", "E200009", "E155010", "W155010"]) { return .notFound }
        if !set.isDisjoint(with: ["E670002", "E670008", "E170013", "E175002", "E175012", "E120171", "E120108", "E000060", "E000061", "E000065", "E731001", "E731004"])
            || lower.contains("unable to connect") || lower.contains("timed out") || lower.contains("could not resolve") {
            return .network
        }
        return .other
    }
}
