import Foundation

/// A clean release candidate: exactly the files that will be published.
public struct StagedCandidate: Codable, Hashable, Sendable {
    public var rootPath: String
    public var entries: [PackageEntry]
    public var treeDigest: String

    public var rootURL: URL { URL(fileURLWithPath: rootPath, isDirectory: true) }
    public var files: [PackageEntry] { entries.filter { $0.kind == .file } }
}

public enum ReleaseError: Error, LocalizedError, Sendable {
    case invalidState(String)
    case productNotFound
    case releaseNotFound
    case validationBlocked(Int)
    case automationNotAuthorized([String])
    case previewChanged
    case remoteChanged(String)
    case releasePathExists(String)
    case candidateChanged(String)
    case workingCopyObstructed([String])
    case credentialsRequired
    case missingData(String)
    case notCommitted
    case readmeUpdateChangesCode(tag: String, paths: [String])

    public var errorDescription: String? {
        switch self {
        case .invalidState(let s): s
        case .productNotFound: "The product no longer exists."
        case .releaseNotFound: "The release record was not found."
        case .validationBlocked(let n): "\(n) required check(s) failed. Resolve them before publishing."
        case .automationNotAuthorized(let reasons): "Automatic publishing is not allowed: " + reasons.joined(separator: "; ")
        case .previewChanged: "The working copy changed after the preview was approved. Review the new preview and approve again."
        case .remoteChanged(let detail): "The repository changed on the server: \(detail). Prepare the release again."
        case .releasePathExists(let path): "\(path) already exists on the server."
        case .candidateChanged(let detail): "The release candidate changed after inspection: \(detail)"
        case .workingCopyObstructed(let paths): "Working-copy paths changed type and need manual attention: \(paths.prefix(5).joined(separator: ", "))"
        case .credentialsRequired: "SVN credentials are required to publish."
        case .missingData(let what): "Release data missing: \(what)"
        case .notCommitted: "The commit did not reach the repository."
        case .readmeUpdateChangesCode(let tag, let paths):
            "The package differs from the released \(tag) in \(paths.count) file(s) besides the readme (\(paths.prefix(5).joined(separator: ", "))\(paths.count > 5 ? ", …" : "")). Publish a new version to ship code changes."
        }
    }
}

public protocol ReleaseStaging: Sendable {
    func workspace(for releaseID: UUID) throws -> URL
    /// Extracts an inspected ZIP into the release workspace and returns the package root.
    func extract(zip: URL, inspection: PackageInspection, releaseID: UUID) throws -> (root: URL, entries: [PackageEntry])
    /// Copies the included files from `sourceRoot` into a fresh candidate directory,
    /// verifying each file against the hash recorded at inspection time.
    func buildCandidate(from sourceRoot: URL, entries: [PackageEntry], releaseID: UUID) throws -> StagedCandidate
    func writeManifest(_ manifest: ReleaseManifest, slug: String, version: String, releaseID: UUID) throws -> URL
    func writeBackup(_ text: String, name: String) throws -> URL
    func removeWorkspace(for releaseID: UUID)
}

public struct ReleaseStagingService: ReleaseStaging {
    public let paths: AppPaths

    public init(paths: AppPaths) {
        self.paths = paths
    }

    public func workspace(for releaseID: UUID) throws -> URL {
        let dir = paths.stagingDirectory(for: releaseID)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
        return dir
    }

    public func extract(zip: URL, inspection: PackageInspection, releaseID: UUID) throws -> (root: URL, entries: [PackageEntry]) {
        let destination = try workspace(for: releaseID).appendingPathComponent("extracted", isDirectory: true)
        if FileManager.default.fileExists(atPath: destination.path) {
            try FileManager.default.removeItem(at: destination)
        }
        let entries = try ZIPInspectionService().extract(zipAt: zip, inspection: inspection, to: destination)
        return (destination, entries)
    }

    public func buildCandidate(from sourceRoot: URL, entries: [PackageEntry], releaseID: UUID) throws -> StagedCandidate {
        let fm = FileManager.default
        let candidate = try workspace(for: releaseID).appendingPathComponent("candidate", isDirectory: true)
        if fm.fileExists(atPath: candidate.path) { try fm.removeItem(at: candidate) }
        try fm.createDirectory(at: candidate, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o755])

        var result: [PackageEntry] = []
        for entry in entries.sorted(by: { $0.path < $1.path }) {
            let components = try PathSafety.validateRelativePath(entry.path)
            let destination = try PathSafety.resolve(components, under: candidate)
            switch entry.kind {
            case .directory:
                try fm.createDirectory(at: destination, withIntermediateDirectories: true)
                result.append(entry)
            case .file:
                let source = try PathSafety.resolve(components, under: sourceRoot)
                let values = try source.resourceValues(forKeys: [.isSymbolicLinkKey, .isRegularFileKey])
                guard values.isRegularFile == true, values.isSymbolicLink != true else {
                    throw ReleaseError.candidateChanged("\(entry.path) is no longer a regular file")
                }
                try fm.createDirectory(at: destination.deletingLastPathComponent(), withIntermediateDirectories: true)
                try fm.copyItem(at: source, to: destination)
                let sha = try Hashing.sha256(ofFile: destination)
                if let expected = entry.sha256, expected != sha {
                    throw ReleaseError.candidateChanged("\(entry.path) was modified after inspection")
                }
                var copied = entry
                copied.sha256 = sha
                copied.size = UInt64((try? fm.attributesOfItem(atPath: destination.path)[.size] as? NSNumber)??.uint64Value ?? 0)
                result.append(copied)
            case .symlink:
                throw ReleaseError.candidateChanged("\(entry.path) is a symbolic link")
            }
        }
        let digest = Hashing.treeDigest(result.filter { $0.kind == .file }.map { ($0.path, $0.sha256 ?? "") })
        return StagedCandidate(rootPath: candidate.path, entries: result, treeDigest: digest)
    }

    public func writeManifest(_ manifest: ReleaseManifest, slug: String, version: String, releaseID: UUID) throws -> URL {
        let dir = paths.releaseDirectory(slug: slug, version: version, releaseID: releaseID)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
        let url = dir.appendingPathComponent("manifest.json")
        try manifest.encoded().write(to: url, options: [.atomic])
        return url
    }

    public func writeBackup(_ text: String, name: String) throws -> URL {
        try FileManager.default.createDirectory(at: paths.backups, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
        let url = paths.backups.appendingPathComponent(name)
        try Data(text.utf8).write(to: url, options: [.atomic])
        return url
    }

    public func removeWorkspace(for releaseID: UUID) {
        try? FileManager.default.removeItem(at: paths.stagingDirectory(for: releaseID))
    }
}
