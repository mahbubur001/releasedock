import Foundation

public enum AuthenticationAvailability: String, Codable, Sendable {
    /// Credentials stored in ReleaseDock's Keychain item.
    case keychain
    /// svn has credentials cached for the repository realm.
    case cachedBySVN
    /// No credentials anywhere; publishing will need them.
    case none
    case unknown
}

/// Facts about the SVN repository and working copy gathered before validation.
/// Validation itself is pure and reads only this snapshot.
public struct RepositorySnapshot: Codable, Hashable, Sendable {
    public var svnVersion: String?
    /// Nil when the repository was reachable.
    public var connectivityError: String?
    public var repositoryExists: Bool?
    /// Released version according to the repository (plugin Stable tag / highest theme version).
    public var currentVersion: String?
    public var currentVersionSource: String?
    public var existingVersions: [String]
    /// Whether the candidate version's tag/directory already exists remotely.
    public var releasePathExists: Bool?
    public var structureProblems: [String]
    public var workingCopyExists: Bool
    public var workingCopyURLMismatch: String?
    public var workingCopyChanges: [String]
    public var workingCopyRevision: Int?
    public var remoteRevision: Int?
    public var outOfDatePaths: [String]
    public var authentication: AuthenticationAvailability
    public var notes: [String]
    /// Title line of the server's trunk readme (plugins), used to name new products.
    public var readmeTitle: String?
    /// Latest stable WordPress release when WordPress.org could be reached.
    public var latestWordPressVersion: String?

    public init(
        svnVersion: String? = nil,
        connectivityError: String? = nil,
        repositoryExists: Bool? = nil,
        currentVersion: String? = nil,
        currentVersionSource: String? = nil,
        existingVersions: [String] = [],
        releasePathExists: Bool? = nil,
        structureProblems: [String] = [],
        workingCopyExists: Bool = false,
        workingCopyURLMismatch: String? = nil,
        workingCopyChanges: [String] = [],
        workingCopyRevision: Int? = nil,
        remoteRevision: Int? = nil,
        outOfDatePaths: [String] = [],
        authentication: AuthenticationAvailability = .unknown,
        notes: [String] = []
    ) {
        self.svnVersion = svnVersion
        self.connectivityError = connectivityError
        self.repositoryExists = repositoryExists
        self.currentVersion = currentVersion
        self.currentVersionSource = currentVersionSource
        self.existingVersions = existingVersions
        self.releasePathExists = releasePathExists
        self.structureProblems = structureProblems
        self.workingCopyExists = workingCopyExists
        self.workingCopyURLMismatch = workingCopyURLMismatch
        self.workingCopyChanges = workingCopyChanges
        self.workingCopyRevision = workingCopyRevision
        self.remoteRevision = remoteRevision
        self.outOfDatePaths = outOfDatePaths
        self.authentication = authentication
        self.notes = notes
    }
}

/// One local change in a product's working copy.
public struct WorkingCopyChange: Hashable, Sendable, Identifiable {
    /// Relative to the working-copy root.
    public var path: String
    public var status: SVNItemStatus
    public var isConflict: Bool
    public var isTreeConflict: Bool

    public var id: String { path }

    public init(path: String, status: SVNItemStatus, isConflict: Bool = false, isTreeConflict: Bool = false) {
        self.path = path
        self.status = status
        self.isConflict = isConflict
        self.isTreeConflict = isTreeConflict
    }
}

/// A product's working copy as the Products page shows it.
public struct WorkingCopyState: Hashable, Sendable {
    public var exists: Bool
    public var revision: Int?
    /// Everything that would fail the "Working copy cleanliness" check.
    public var changes: [WorkingCopyChange]

    public var conflicts: [WorkingCopyChange] { changes.filter(\.isConflict) }
    public var isClean: Bool { changes.isEmpty }

    public init(exists: Bool, revision: Int? = nil, changes: [WorkingCopyChange] = []) {
        self.exists = exists
        self.revision = revision
        self.changes = changes
    }
}

/// Which side wins when resolving a conflict.
public enum ConflictResolution: String, Sendable, CaseIterable {
    /// Take the WordPress.org version; the local change is backed up first.
    case server
    /// Keep the local version.
    case mine
}
