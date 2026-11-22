import Foundation

public enum SourceKind: String, Codable, Sendable {
    case zip
    case folder
}

public enum EntryKind: String, Codable, Sendable {
    case file
    case directory
    case symlink
}

public struct PackageEntry: Codable, Hashable, Sendable {
    /// Path relative to the package root, `/`-separated.
    public var path: String
    public var kind: EntryKind
    public var size: UInt64
    public var sha256: String?

    public init(path: String, kind: EntryKind, size: UInt64, sha256: String? = nil) {
        self.path = path
        self.kind = kind
        self.size = size
        self.sha256 = sha256
    }
}

public enum InspectionIssueKind: String, Codable, Sendable {
    case pathTraversal
    case absolutePath
    case unsafeFilename
    case symbolicLink
    case duplicatePath
    case nestedRoot
    case multipleRoots
    case encryptedEntry
    case unsupportedCompression
    case corruptArchive
    case archiveTooLarge
    case suspiciousCompressionRatio
    case junkFile
    case versionControlMetadata
    case developmentArtifact
    case forbiddenFile
    case possibleSecret
    case zeroByteFile
    case portability
    case specialFile
}

public struct InspectionIssue: Codable, Hashable, Sendable, Identifiable {
    public var id: String { "\(kind.rawValue):\(path ?? ""):\(message)" }
    public var kind: InspectionIssueKind
    public var severity: ValidationSeverity
    public var path: String?
    public var message: String
    /// Whether the path is excluded from the release candidate automatically.
    public var excluded: Bool

    public init(kind: InspectionIssueKind, severity: ValidationSeverity, path: String?, message: String, excluded: Bool = false) {
        self.kind = kind
        self.severity = severity
        self.path = path
        self.message = message
        self.excluded = excluded
    }
}

/// Result of inspecting a ZIP archive or source folder. Inspection never modifies the source.
public struct PackageInspection: Codable, Hashable, Sendable {
    public var sourceKind: SourceKind
    public var sourcePath: String
    /// SHA-256 of the ZIP file, or the tree digest of the included files for a folder.
    public var sha256: String
    /// Top-level directory inside the archive, when the archive wraps the package in one.
    public var archiveRootName: String?
    /// Entries that will form the release candidate (after exclusions), relative to the package root.
    public var includedEntries: [PackageEntry]
    /// Paths excluded from the release candidate, relative to the package root.
    public var excludedPaths: [String]
    public var issues: [InspectionIssue]
    public var totalUncompressedSize: UInt64
    public var inspectedAt: Date

    public init(
        sourceKind: SourceKind,
        sourcePath: String,
        sha256: String,
        archiveRootName: String?,
        includedEntries: [PackageEntry],
        excludedPaths: [String],
        issues: [InspectionIssue],
        totalUncompressedSize: UInt64,
        inspectedAt: Date = Date()
    ) {
        self.sourceKind = sourceKind
        self.sourcePath = sourcePath
        self.sha256 = sha256
        self.archiveRootName = archiveRootName
        self.includedEntries = includedEntries
        self.excludedPaths = excludedPaths
        self.issues = issues
        self.totalUncompressedSize = totalUncompressedSize
        self.inspectedAt = inspectedAt
    }

    public var blockingIssues: [InspectionIssue] { issues.filter { $0.severity == .error && !$0.excluded } }
    public var includedFiles: [PackageEntry] { includedEntries.filter { $0.kind == .file } }
}
