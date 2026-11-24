import Foundation

public enum ValidationCheckID: String, Codable, CaseIterable, Sendable, Identifiable {
    case sourceIntegrity
    case productIdentity
    case versionValidity
    case versionIncrease
    case metadataValidity
    case requiredFiles
    case forbiddenFiles
    case packageStructure
    case svnConnectivity
    case workingCopyClean
    case existingTag
    case remoteFreshness
    case releaseDiff
    case authentication
    case releasedVersion
    // Optional checks
    case changelogEntry
    case testedUpTo
    case packageSize
    case gitWorkingTree
    case portableFilenames
    case phpSyntax
    case pluginCheck
    case directoryAssets

    public var id: String { rawValue }

    /// Required checks always run and their failures always block a release.
    public var isRequired: Bool {
        switch self {
        case .changelogEntry, .testedUpTo, .packageSize, .gitWorkingTree, .portableFilenames, .phpSyntax, .pluginCheck, .directoryAssets: false
        default: true
        }
    }

    public var title: String {
        switch self {
        case .sourceIntegrity: "Source integrity"
        case .productIdentity: "Product identity"
        case .versionValidity: "Version validity"
        case .versionIncrease: "Version increase"
        case .metadataValidity: "Metadata validity"
        case .requiredFiles: "Required files"
        case .forbiddenFiles: "Forbidden files"
        case .packageStructure: "Package structure"
        case .svnConnectivity: "SVN connectivity"
        case .workingCopyClean: "Working copy cleanliness"
        case .existingTag: "Existing release tag"
        case .remoteFreshness: "Remote repository freshness"
        case .releaseDiff: "Release diff"
        case .authentication: "Authentication"
        case .releasedVersion: "Released version"
        case .changelogEntry: "Changelog entry"
        case .testedUpTo: "Tested up to"
        case .packageSize: "Package size"
        case .gitWorkingTree: "Git working tree"
        case .portableFilenames: "Portable file names"
        case .phpSyntax: "PHP syntax"
        case .pluginCheck: "Plugin Check"
        case .directoryAssets: "Directory assets"
        }
    }
}

public enum ValidationStatus: String, Codable, Sendable {
    case passed
    case warning
    case failed
    case skipped
}

public enum ValidationSeverity: String, Codable, Sendable, Comparable {
    case info
    case warning
    case error

    private var rank: Int {
        switch self {
        case .info: 0
        case .warning: 1
        case .error: 2
        }
    }

    public static func < (lhs: Self, rhs: Self) -> Bool { lhs.rank < rhs.rank }
}

public struct ValidationResult: Codable, Hashable, Sendable, Identifiable {
    public var id: String { "\(check.rawValue):\(filePath ?? ""):\(message)" }
    public var check: ValidationCheckID
    public var name: String
    public var status: ValidationStatus
    public var severity: ValidationSeverity
    public var message: String
    public var filePath: String?
    public var suggestion: String?

    public init(
        check: ValidationCheckID,
        status: ValidationStatus,
        severity: ValidationSeverity,
        message: String,
        filePath: String? = nil,
        suggestion: String? = nil
    ) {
        self.check = check
        self.name = check.title
        self.status = status
        self.severity = severity
        self.message = message
        self.filePath = filePath
        self.suggestion = suggestion
    }

    public static func pass(_ check: ValidationCheckID, _ message: String) -> Self {
        .init(check: check, status: .passed, severity: .info, message: message)
    }

    public static func fail(_ check: ValidationCheckID, _ message: String, file: String? = nil, fix: String? = nil) -> Self {
        .init(check: check, status: .failed, severity: .error, message: message, filePath: file, suggestion: fix)
    }

    public static func warn(_ check: ValidationCheckID, _ message: String, file: String? = nil, fix: String? = nil) -> Self {
        .init(check: check, status: .warning, severity: .warning, message: message, filePath: file, suggestion: fix)
    }

    public static func skip(_ check: ValidationCheckID, _ message: String) -> Self {
        .init(check: check, status: .skipped, severity: .info, message: message)
    }
}

public struct ValidationReport: Codable, Hashable, Sendable {
    public var results: [ValidationResult]
    public var blockOnWarnings: Bool

    public init(results: [ValidationResult], blockOnWarnings: Bool = false) {
        self.results = results
        self.blockOnWarnings = blockOnWarnings
    }

    /// Results that prevent publishing.
    public var blockingResults: [ValidationResult] {
        results.filter { result in
            switch result.status {
            case .failed: true
            case .warning: blockOnWarnings
            case .passed: false
            case .skipped: result.check.isRequired && result.check.blocksWhenSkipped
            }
        }
    }

    public var isBlocking: Bool { !blockingResults.isEmpty }
    public var warnings: [ValidationResult] { results.filter { $0.status == .warning } }
    public var failures: [ValidationResult] { results.filter { $0.status == .failed } }
}

extension ValidationCheckID {
    /// A required check that could not run (e.g. SVN unreachable) must not silently pass.
    var blocksWhenSkipped: Bool {
        switch self {
        case .releaseDiff: false // Only computed after staging.
        default: true
        }
    }
}
