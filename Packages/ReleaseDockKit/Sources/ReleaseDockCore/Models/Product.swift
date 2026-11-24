import Foundation

public enum ProductType: String, Codable, CaseIterable, Sendable, Identifiable {
    case plugin
    case theme

    public var id: String { rawValue }
    public var displayName: String { self == .plugin ? "Plugin" : "Theme" }

    /// Default WordPress.org SVN repository for a slug.
    public func defaultRepositoryURL(slug: String) -> URL? {
        switch self {
        case .plugin: URL(string: "https://plugins.svn.wordpress.org/\(slug)")
        case .theme: URL(string: "https://themes.svn.wordpress.org/\(slug)")
        }
    }
}

public enum ReleaseMode: String, Codable, CaseIterable, Sendable, Identifiable {
    /// Prepare and validate everything without committing.
    case dryRun
    /// Run every check and require a final user confirmation. Default.
    case assisted
    /// Publish without confirmation, only when every automation precondition holds.
    case automatic

    public var id: String { rawValue }
    public var displayName: String {
        switch self {
        case .dryRun: "Dry Run"
        case .assisted: "Assisted Release"
        case .automatic: "Automatic Release"
        }
    }
}

/// Per-product validation configuration. Required checks cannot be disabled;
/// only checks whose `ValidationCheckID.isRequired` is false may be listed in
/// `disabledChecks`.
public struct ValidationSettings: Codable, Hashable, Sendable {
    /// Treat warnings as blocking.
    public var blockOnWarnings: Bool
    /// Require a readme changelog entry for the release version (plugins).
    public var requireChangelogEntry: Bool
    /// Require the local development Git working tree to be clean.
    public var requireCleanGitTree: Bool
    /// Additional glob patterns excluded from the release candidate.
    public var excludePatterns: [String]
    /// Optional checks the user turned off.
    public var disabledChecks: Set<ValidationCheckID>
    /// Package size above which a warning is raised, in megabytes.
    public var packageSizeWarningMB: Int

    public init(
        blockOnWarnings: Bool = false,
        requireChangelogEntry: Bool = true,
        requireCleanGitTree: Bool = false,
        excludePatterns: [String] = [],
        disabledChecks: Set<ValidationCheckID> = [],
        packageSizeWarningMB: Int = 10
    ) {
        self.blockOnWarnings = blockOnWarnings
        self.requireChangelogEntry = requireChangelogEntry
        self.requireCleanGitTree = requireCleanGitTree
        self.excludePatterns = excludePatterns
        self.disabledChecks = disabledChecks
        self.packageSizeWarningMB = packageSizeWarningMB
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        let d = ValidationSettings()
        blockOnWarnings = try c.decodeIfPresent(Bool.self, forKey: .blockOnWarnings) ?? d.blockOnWarnings
        requireChangelogEntry = try c.decodeIfPresent(Bool.self, forKey: .requireChangelogEntry) ?? d.requireChangelogEntry
        requireCleanGitTree = try c.decodeIfPresent(Bool.self, forKey: .requireCleanGitTree) ?? d.requireCleanGitTree
        excludePatterns = try c.decodeIfPresent([String].self, forKey: .excludePatterns) ?? d.excludePatterns
        // Unknown check identifiers from newer versions are dropped rather than failing decoding.
        let rawDisabled = try c.decodeIfPresent([String].self, forKey: .disabledChecks) ?? []
        disabledChecks = Set(rawDisabled.compactMap(ValidationCheckID.init(rawValue:)))
        packageSizeWarningMB = try c.decodeIfPresent(Int.self, forKey: .packageSizeWarningMB) ?? d.packageSizeWarningMB
    }
}

public struct ReleaseSummary: Codable, Hashable, Sendable {
    public var releaseID: UUID
    public var version: String
    public var revision: Int?
    public var date: Date

    public init(releaseID: UUID, version: String, revision: Int?, date: Date) {
        self.releaseID = releaseID
        self.version = version
        self.revision = revision
        self.date = date
    }
}

public struct Product: Codable, Identifiable, Hashable, Sendable {
    public var id: UUID
    public var name: String
    public var slug: String
    public var type: ProductType
    /// Header name expected in the package (`Plugin Name:` / `Theme Name:`), used for identity checks.
    public var expectedHeaderName: String
    public var localDevelopmentPath: String?
    public var svnRepositoryURL: URL
    public var workingCopyPath: String
    /// Last known released version on SVN.
    public var currentVersion: String?
    public var lastSuccessfulRelease: ReleaseSummary?
    public var releaseMode: ReleaseMode
    /// Explicit opt-in required in addition to `releaseMode == .automatic`.
    public var automationEnabled: Bool
    public var validation: ValidationSettings
    public var githubRepositoryURL: URL?
    /// Main plugin file relative to the package root, when auto-detection is ambiguous.
    public var mainPluginFile: String?
    /// Optional local folder mirrored to the plugin SVN `/assets` directory.
    public var assetsPath: String?
    /// Shell command run on a copy of a folder source before inspection, e.g.
    /// `composer install --no-dev && npm run build`. Not used for ZIP sources.
    public var buildCommand: String?
    /// Opt-in GitHub release after each version release; needs `githubRepositoryURL`.
    public var githubRelease: GitHubReleaseSettings?
    public var svnUsername: String?
    public var createdAt: Date
    public var updatedAt: Date

    public init(
        id: UUID = UUID(),
        name: String,
        slug: String,
        type: ProductType,
        expectedHeaderName: String? = nil,
        localDevelopmentPath: String? = nil,
        svnRepositoryURL: URL,
        workingCopyPath: String,
        currentVersion: String? = nil,
        lastSuccessfulRelease: ReleaseSummary? = nil,
        releaseMode: ReleaseMode = .assisted,
        automationEnabled: Bool = false,
        validation: ValidationSettings = ValidationSettings(),
        githubRepositoryURL: URL? = nil,
        mainPluginFile: String? = nil,
        assetsPath: String? = nil,
        buildCommand: String? = nil,
        githubRelease: GitHubReleaseSettings? = nil,
        svnUsername: String? = nil,
        createdAt: Date = Date(),
        updatedAt: Date = Date()
    ) {
        self.id = id
        self.name = name
        self.slug = slug
        self.type = type
        self.expectedHeaderName = expectedHeaderName ?? name
        self.localDevelopmentPath = localDevelopmentPath
        self.svnRepositoryURL = svnRepositoryURL
        self.workingCopyPath = workingCopyPath
        self.currentVersion = currentVersion
        self.lastSuccessfulRelease = lastSuccessfulRelease
        self.releaseMode = releaseMode
        self.automationEnabled = automationEnabled
        self.validation = validation
        self.githubRepositoryURL = githubRepositoryURL
        self.mainPluginFile = mainPluginFile
        self.assetsPath = assetsPath
        self.buildCommand = buildCommand
        self.githubRelease = githubRelease
        self.svnUsername = svnUsername
        self.createdAt = createdAt
        self.updatedAt = updatedAt
    }

    /// The trimmed build command, or nil when none is set.
    public var effectiveBuildCommand: String? {
        let command = buildCommand?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        return command.isEmpty ? nil : command
    }

    public var workingCopyURL: URL { URL(fileURLWithPath: workingCopyPath, isDirectory: true) }
    public var localDevelopmentURL: URL? { localDevelopmentPath.map { URL(fileURLWithPath: $0, isDirectory: true) } }

    /// Automatic publishing is only possible when both the mode and the explicit opt-in are set.
    public var isAutomaticPublishingAuthorized: Bool { releaseMode == .automatic && automationEnabled }
}
