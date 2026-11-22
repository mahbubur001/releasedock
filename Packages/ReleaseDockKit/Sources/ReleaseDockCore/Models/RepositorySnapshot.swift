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
