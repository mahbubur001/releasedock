import Foundation

public enum ReleaseState: String, Codable, CaseIterable, Sendable {
    case detected
    case inspected
    case validated
    case staged
    case previewed
    case approved
    case publishing
    case verifying
    case completed
    case failed
    case cancelled
    case recoveryRequired

    public var isTerminal: Bool { self == .completed || self == .failed || self == .cancelled }

    /// States in which a commit may have reached the server.
    public var mayHaveCommitted: Bool { self == .publishing || self == .verifying || self == .recoveryRequired }

    public var displayName: String {
        switch self {
        case .detected: "Detected"
        case .inspected: "Inspected"
        case .validated: "Validated"
        case .staged: "Staged"
        case .previewed: "Previewed"
        case .approved: "Approved"
        case .publishing: "Publishing"
        case .verifying: "Verifying"
        case .completed: "Completed"
        case .failed: "Failed"
        case .cancelled: "Cancelled"
        case .recoveryRequired: "Recovery Required"
        }
    }
}

public enum ReleaseSource: Codable, Hashable, Sendable {
    case zip(path: String)
    case folder(path: String)

    public var path: String {
        switch self {
        case .zip(let path), .folder(let path): path
        }
    }

    public var kind: SourceKind {
        switch self {
        case .zip: .zip
        case .folder: .folder
        }
    }
}

public struct StateTransition: Codable, Hashable, Sendable {
    public var from: ReleaseState?
    public var to: ReleaseState
    public var at: Date
    public var note: String?

    public init(from: ReleaseState?, to: ReleaseState, at: Date = Date(), note: String? = nil) {
        self.from = from
        self.to = to
        self.at = at
        self.note = note
    }
}

/// What a release publishes.
public enum ReleaseKind: String, Codable, CaseIterable, Sendable, Identifiable {
    /// A new version: trunk is updated and a new tag (or theme version directory) is created.
    case version
    /// Plugins only: the readme (and `/assets`) of the released version are updated in trunk
    /// and in its tag without a new version, e.g. to raise "Tested up to". The code must be
    /// identical to the released tag.
    case readmeUpdate

    public var id: String { rawValue }

    public var displayName: String {
        switch self {
        case .version: "New Version"
        case .readmeUpdate: "Readme & Assets Update"
        }
    }
}

public enum ApprovalKind: String, Codable, Sendable {
    case user
    case automatic
}

public struct Approval: Codable, Hashable, Sendable {
    public var kind: ApprovalKind
    public var at: Date
    /// Preview fingerprint the approval applies to. Publishing is refused if it changes.
    public var previewFingerprint: String

    public init(kind: ApprovalKind, at: Date = Date(), previewFingerprint: String) {
        self.kind = kind
        self.at = at
        self.previewFingerprint = previewFingerprint
    }
}

public enum ChangeKind: String, Codable, Sendable {
    case added
    case modified
    case deleted
    case replaced
    case propertyChanged
}

public struct PreviewChange: Codable, Hashable, Sendable, Identifiable {
    public var id: String { path }
    /// Path relative to the SVN working-copy root (e.g. `trunk/readme.txt`, `tags/1.2.0`).
    public var path: String
    public var kind: ChangeKind
    public var isDirectory: Bool
    /// True for the tag (or theme version directory) created by copy.
    public var isCopy: Bool

    public init(path: String, kind: ChangeKind, isDirectory: Bool, isCopy: Bool = false) {
        self.path = path
        self.kind = kind
        self.isDirectory = isDirectory
        self.isCopy = isCopy
    }
}

public struct ReleasePreview: Codable, Hashable, Sendable {
    public var changes: [PreviewChange]
    /// Unified diff text of modified text files, truncated for display.
    public var diffExcerpt: String
    public var diffTruncated: Bool
    public var commitMessage: String
    /// Digest over the change list and staged file hashes; approvals bind to it.
    public var fingerprint: String
    public var workingCopyRevision: Int?
    public var createdAt: Date

    public init(changes: [PreviewChange], diffExcerpt: String, diffTruncated: Bool, commitMessage: String, fingerprint: String, workingCopyRevision: Int?, createdAt: Date = Date()) {
        self.changes = changes
        self.diffExcerpt = diffExcerpt
        self.diffTruncated = diffTruncated
        self.commitMessage = commitMessage
        self.fingerprint = fingerprint
        self.workingCopyRevision = workingCopyRevision
        self.createdAt = createdAt
    }

    public var added: Int { changes.filter { $0.kind == .added && !$0.isDirectory }.count }
    public var modified: Int { changes.filter { $0.kind == .modified || $0.kind == .replaced }.count }
    public var deleted: Int { changes.filter { $0.kind == .deleted }.count }
}

public struct VerificationReport: Codable, Hashable, Sendable {
    public var committedRevision: Int?
    public var tagExists: Bool
    public var tagURL: String
    public var missingFiles: [String]
    public var unexpectedFiles: [String]
    public var checksumMismatches: [String]
    public var stableTagMatches: Bool?
    public var notes: [String]

    public init(committedRevision: Int?, tagExists: Bool, tagURL: String, missingFiles: [String] = [], unexpectedFiles: [String] = [], checksumMismatches: [String] = [], stableTagMatches: Bool? = nil, notes: [String] = []) {
        self.committedRevision = committedRevision
        self.tagExists = tagExists
        self.tagURL = tagURL
        self.missingFiles = missingFiles
        self.unexpectedFiles = unexpectedFiles
        self.checksumMismatches = checksumMismatches
        self.stableTagMatches = stableTagMatches
        self.notes = notes
    }

    public var isClean: Bool {
        tagExists && missingFiles.isEmpty && unexpectedFiles.isEmpty && checksumMismatches.isEmpty && stableTagMatches != false
    }
}

/// Whether WordPress.org offers a committed release to sites yet. A commit is not
/// distributed immediately: the directory holds releases for a security cooldown,
/// and Release Confirmation (when enabled) waits for a committer.
public struct DirectoryStatus: Codable, Hashable, Sendable {
    public enum State: String, Codable, Sendable {
        /// The directory still offers an older version.
        case pending
        /// The directory offers this release.
        case live
        /// The directory already offers a newer version.
        case superseded
        /// The directory has no listing for the slug.
        case notListed
    }

    public var state: State
    public var listedVersion: String?
    public var checkedAt: Date
    public var liveSince: Date?

    public init(state: State, listedVersion: String?, checkedAt: Date = Date(), liveSince: Date? = nil) {
        self.state = state
        self.listedVersion = listedVersion
        self.checkedAt = checkedAt
        self.liveSince = liveSince
    }

    public static func state(released: String, listing: DirectoryListing?) -> State {
        guard let listing else { return .notListed }
        guard let listed = listing.version else { return .pending }
        let order = PHPVersion.compare(listed, released)
        return order == 0 ? .live : order > 0 ? .superseded : .pending
    }
}

/// Persisted record of one release attempt. Everything needed to resume, recover or
/// report the release lives here; credentials never do.
public struct ReleaseRecord: Codable, Hashable, Sendable, Identifiable {
    public var id: UUID
    public var productID: UUID
    public var productName: String
    public var productSlug: String
    public var productType: ProductType
    public var source: ReleaseSource
    public var mode: ReleaseMode
    /// Nil in records created before readme updates existed; read through `kind`.
    public var releaseKind: ReleaseKind?
    public var state: ReleaseState
    public var version: String?
    public var previousVersion: String?
    public var sourceSHA256: String?
    public var svnURL: String
    public var inspection: PackageInspection?
    public var detected: DetectedProduct?
    public var validation: ValidationReport?
    public var preview: ReleasePreview?
    public var approval: Approval?
    public var revisionBefore: Int?
    public var committedRevision: Int?
    public var verification: VerificationReport?
    public var transitions: [StateTransition]
    public var errors: [String]
    public var manifestPath: String?
    public var stagingPath: String?
    public var candidate: StagedCandidate?
    public var repository: RepositorySnapshot?
    public var gitCommit: String?
    /// The release changed the SVN working copy (staged files, scheduled copy).
    public var workingCopyModified: Bool
    /// Recovery inspected the repository and proved the commit did not land.
    public var verifiedNotCommitted: Bool
    /// Post-release instructions for the user (e.g. Release Confirmation).
    public var guidance: [String]
    /// PHP lint and Plugin Check results for the staged candidate.
    public var codeChecks: CodeCheckReport?
    /// Last WordPress.org directory check after the commit.
    public var directoryStatus: DirectoryStatus?
    public var createdAt: Date
    public var updatedAt: Date

    public init(
        id: UUID = UUID(),
        product: Product,
        source: ReleaseSource,
        mode: ReleaseMode,
        kind: ReleaseKind = .version,
        createdAt: Date = Date()
    ) {
        self.id = id
        releaseKind = kind
        productID = product.id
        productName = product.name
        productSlug = product.slug
        productType = product.type
        self.source = source
        self.mode = mode
        state = .detected
        previousVersion = product.currentVersion
        svnURL = product.svnRepositoryURL.absoluteString
        transitions = [StateTransition(from: nil, to: .detected, at: createdAt)]
        errors = []
        workingCopyModified = false
        verifiedNotCommitted = false
        guidance = []
        self.createdAt = createdAt
        updatedAt = createdAt
    }

    /// Marker embedded in the commit message so a release can be recognised on the
    /// server after an uncertain commit (timeout, crash, cancelled process).
    public var commitMarker: String { "ReleaseDock-Release-ID: \(id.uuidString)" }

    public var kind: ReleaseKind { releaseKind ?? .version }

    public var completedAt: Date? { transitions.last { $0.to == .completed }?.at }

    /// Version releases committed to WordPress.org can be followed until the directory
    /// offers them. Readme updates keep the version, so there is nothing to wait for.
    public var isDirectoryTracked: Bool {
        guard state == .completed, mode != .dryRun, kind == .version, let host = URL(string: svnURL)?.host?.lowercased() else { return false }
        return host == "plugins.svn.wordpress.org" || host == "themes.svn.wordpress.org"
    }

    /// Still worth checking: tracked, not live or superseded yet, and recent.
    public func awaitsDirectory(now: Date = Date(), window: TimeInterval = 72 * 3600) -> Bool {
        guard isDirectoryTracked, let completedAt, now.timeIntervalSince(completedAt) < window else { return false }
        switch directoryStatus?.state {
        case .live?, .superseded?: return false
        case .pending?, .notListed?, nil: return true
        }
    }
}
