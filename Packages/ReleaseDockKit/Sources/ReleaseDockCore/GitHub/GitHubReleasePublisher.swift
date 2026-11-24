import Foundation

/// Per-product opt-in for a GitHub release after each WordPress.org release.
public struct GitHubReleaseSettings: Codable, Hashable, Sendable {
    public var enabled: Bool
    /// Prepended to the version for the tag name, e.g. `v` → `v1.2.0`.
    public var tagPrefix: String
    public var draft: Bool

    public init(enabled: Bool = false, tagPrefix: String = "v", draft: Bool = false) {
        self.enabled = enabled
        self.tagPrefix = tagPrefix
        self.draft = draft
    }
}

public struct GitHubReleaseResult: Codable, Hashable, Sendable {
    public var tag: String
    public var url: URL?
    public var assetName: String?
    /// The release for the tag existed before (an earlier attempt or made by hand).
    public var alreadyExisted: Bool
    public var notes: [String]
    public var error: String?
    public var at: Date

    public init(tag: String, url: URL? = nil, assetName: String? = nil, alreadyExisted: Bool = false, notes: [String] = [], error: String? = nil, at: Date = Date()) {
        self.tag = tag
        self.url = url
        self.assetName = assetName
        self.alreadyExisted = alreadyExisted
        self.notes = notes
        self.error = error
        self.at = at
    }

    public var succeeded: Bool { error == nil && url != nil }
}

/// Creates the GitHub release for a completed WordPress.org release. The tag is created by
/// GitHub on the released commit (`target_commitish`), so the local repository is never
/// written to. Re-running reuses the release for the tag instead of creating a second one.
public struct GitHubReleasePublisher: Sendable {
    private let github: GitHubReleasing

    public init(github: GitHubReleasing) {
        self.github = github
    }

    public func publish(record: ReleaseRecord, product: Product, zip: URL?, token: String?) async -> GitHubReleaseResult {
        let settings = product.githubRelease ?? GitHubReleaseSettings()
        let version = record.version ?? ""
        let tag = settings.tagPrefix + version
        func failure(_ message: String) -> GitHubReleaseResult { GitHubReleaseResult(tag: tag, error: message) }

        guard let repoURL = product.githubRepositoryURL, let repository = GitHubRepository(string: repoURL.absoluteString) else {
            return failure("Set the product's GitHub repository first.")
        }
        guard let token, !token.isEmpty else {
            return failure("Add a GitHub token for releases in Settings (fine-grained, Contents: read and write).")
        }
        guard let commit = record.gitCommit else {
            return failure("GitHub releases need the released commit: release from the Git development folder, not a ZIP.")
        }
        var notes: [String] = []
        if record.validation?.results.contains(where: { $0.check == .gitWorkingTree && $0.status != .passed }) == true {
            notes.append("The development folder had uncommitted changes when \(version) was released; the tag points at \(commit.prefix(10)).")
        }
        let assetName = "\(product.slug)-\(version).zip"
        do {
            if let existing = try await github.release(tag: tag, in: repository, token: token) {
                var uploaded: String?
                if let zip, !existing.assets.contains(where: { $0.name == assetName }) {
                    try await github.uploadAsset(zip, name: assetName, to: existing, token: token)
                    uploaded = assetName
                }
                return GitHubReleaseResult(tag: tag, url: existing.htmlURL, assetName: uploaded ?? existing.assets.first { $0.name == assetName }?.name, alreadyExisted: true, notes: notes)
            }
            guard try await github.commitExists(commit, in: repository, token: token) else {
                throw GitHubError.commitNotOnGitHub(commit)
            }
            let isPrerelease: Bool = if case .valid(true) = VersionService().validate(version) { true } else { false }
            let release = try await github.createRelease(
                in: repository, tag: tag, commit: commit, name: version,
                body: Self.body(record: record, version: version), draft: settings.draft, prerelease: isPrerelease, token: token
            )
            if let zip {
                try await github.uploadAsset(zip, name: assetName, to: release, token: token)
            } else {
                notes.append("The release ZIP was no longer available, so no asset was attached.")
            }
            return GitHubReleaseResult(tag: tag, url: release.htmlURL, assetName: zip == nil ? nil : assetName, notes: notes)
        } catch {
            var result = failure(error.localizedDescription)
            result.notes = notes
            return result
        }
    }

    static func body(record: ReleaseRecord, version: String) -> String {
        var parts: [String] = []
        if let entry = record.detected?.readme?.changelogEntry(for: version) { parts.append(entry) }
        let revision = record.committedRevision.map { " in SVN revision r\($0)" } ?? ""
        parts.append("Published to WordPress.org\(revision) with ReleaseDock.")
        return parts.joined(separator: "\n\n")
    }
}
