import Foundation

/// Product-type specific release behaviour. Plugins and themes use different SVN
/// layouts and rules, so nothing here is shared implicitly between them.
public protocol ReleaseStrategy: Sendable {
    var productType: ProductType { get }

    /// The directory a release creates, relative to the working-copy root
    /// (`tags/1.2.0` for plugins, `1.2.0` for themes).
    func releasePath(version: String) -> String
    /// Working-copy-relative prefixes a release may change.
    func allowedChangePrefixes(version: String) -> [String]

    // Validation (pure)
    func validateMetadata(detected: DetectedProduct, product: Product) -> [ValidationResult]
    func validateRequiredFiles(inspection: PackageInspection, detected: DetectedProduct) -> [ValidationResult]
    func validateForbiddenFiles(inspection: PackageInspection) -> [ValidationResult]
    func validateVersionFormat(_ version: String) -> [ValidationResult]

    // Repository
    /// Reads remote state: structure, released version, existing versions, and whether
    /// `candidateVersion`'s release path exists.
    func inspectRemote(product: Product, candidateVersion: String?, svn: SVNServicing) async throws -> RepositorySnapshot
    /// Checks out (sparsely) or updates the working copy so it is ready for staging.
    /// `currentVersion` is the latest released version (themes copy from it).
    func prepareWorkingCopy(product: Product, currentVersion: String?, svn: SVNServicing) async throws
    /// Applies the clean candidate to the working copy and schedules the release copy.
    func apply(candidate: StagedCandidate, version: String, currentVersion: String?, product: Product, svn: SVNServicing) async throws
    /// Verifies the committed release on the server.
    func verify(candidate: StagedCandidate, version: String, product: Product, svn: SVNServicing, scratch: URL) async throws -> VerificationReport
    /// Message lines shown after a successful release (e.g. Release Confirmation guidance).
    func postReleaseGuidance(product: Product, version: String) -> [String]

    // Readme updates (no new version)
    /// Writes the candidate's readme into trunk and the released version's tag, and syncs
    /// assets. Throws `ReleaseError.readmeUpdateChangesCode` when anything else in the
    /// candidate differs from the released tag.
    func applyReadmeUpdate(candidate: StagedCandidate, readmeFile: String, version: String, product: Product, svn: SVNServicing) async throws
    /// Verifies that the server's trunk and tag readmes match the candidate's.
    func verifyReadmeUpdate(candidate: StagedCandidate, readmeFile: String, version: String, product: Product, svn: SVNServicing) async throws -> VerificationReport
    func postReadmeUpdateGuidance(product: Product, version: String) -> [String]
}

extension ReleaseStrategy {
    public func applyReadmeUpdate(candidate: StagedCandidate, readmeFile: String, version: String, product: Product, svn: SVNServicing) async throws {
        throw ReleaseError.invalidState("Readme updates without a new version are only available for plugins.")
    }

    public func verifyReadmeUpdate(candidate: StagedCandidate, readmeFile: String, version: String, product: Product, svn: SVNServicing) async throws -> VerificationReport {
        throw ReleaseError.invalidState("Readme updates without a new version are only available for plugins.")
    }

    public func postReadmeUpdateGuidance(product: Product, version: String) -> [String] { [] }
}

public struct StrategyProvider: Sendable {
    public init() {}

    public func strategy(for type: ProductType) -> ReleaseStrategy {
        switch type {
        case .plugin: PluginReleaseStrategy()
        case .theme: ThemeReleaseStrategy()
        }
    }
}

/// Shared working-copy helpers used by both strategies.
enum WorkingCopyOperations {
    /// Makes `target` contain exactly the candidate files (ignoring `.svn`), then schedules
    /// unversioned paths for addition and missing paths for deletion.
    static func mirror(candidate: StagedCandidate, into target: URL, svn: SVNServicing) async throws {
        // Paths that change between file and directory must be scheduled for deletion first;
        // replacing them on disk alone leaves svn with an "obstructed" node.
        let typeChanges = FileMirror.typeChanges(entries: candidate.entries, in: target)
        if !typeChanges.isEmpty {
            try await svn.delete(typeChanges)
        }
        try FileMirror.mirror(source: candidate.rootURL, entries: candidate.entries, into: target)
        if !typeChanges.isEmpty {
            try await svn.add(typeChanges)
        }
        try await scheduleAddsAndDeletes(in: target, svn: svn)
    }

    static func scheduleAddsAndDeletes(in target: URL, svn: SVNServicing) async throws {
        let status = try await svn.status(target, showUpdates: false)
        let obstructed = status.filter { $0.item == .obstructed }
        if !obstructed.isEmpty {
            throw ReleaseError.workingCopyObstructed(obstructed.map(\.path))
        }
        let unversioned = status.filter { $0.item == .unversioned || $0.item == .ignored }.map { URL(fileURLWithPath: $0.path) }
        let missing = status.filter { $0.item == .missing }.map { URL(fileURLWithPath: $0.path) }
        try await svn.add(unversioned)
        try await svn.delete(missing)
    }

    /// Lists all versioned files under a remote URL and compares them (and their SHA-256
    /// after export) with the candidate.
    static func verifyRemoteTree(url: String, candidate: StagedCandidate, svn: SVNServicing, scratch: URL) async throws -> (missing: [String], unexpected: [String], mismatched: [String]) {
        let listing = try await svn.list(url, recursive: true)
        let remoteFiles = Set(listing.filter { $0.kind == .file }.map(\.name))
        let expected = Set(candidate.entries.filter { $0.kind == .file }.map(\.path))
        let missing = expected.subtracting(remoteFiles).sorted()
        let unexpected = remoteFiles.subtracting(expected).sorted()

        let exportDir = scratch.appendingPathComponent("verify-\(UUID().uuidString)", isDirectory: true)
        try? FileManager.default.removeItem(at: exportDir)
        try await svn.export(url, to: exportDir)
        defer { try? FileManager.default.removeItem(at: exportDir) }
        var mismatched: [String] = []
        for entry in candidate.entries where entry.kind == .file && remoteFiles.contains(entry.path) {
            let file = exportDir.appendingPathComponent(entry.path)
            let sha = try? Hashing.sha256(ofFile: file)
            if sha != entry.sha256 { mismatched.append(entry.path) }
        }
        return (missing, unexpected, mismatched)
    }

    /// `svn cat` that returns nil only when the file is positively absent; other failures throw.
    static func catIfExists(_ url: String, svn: SVNServicing) async throws -> Data? {
        do {
            return try await svn.cat(url)
        } catch let error as SVNError where error.kind == .notFound {
            return nil
        }
    }

    /// Returns a mismatch description if `path` is a working copy of a different URL.
    static func workingCopyMismatch(at path: URL, expected: URL, svn: SVNServicing) async throws -> String? {
        let svnDir = path.appendingPathComponent(".svn")
        guard FileManager.default.fileExists(atPath: svnDir.path) else {
            let contents = (try? FileManager.default.contentsOfDirectory(atPath: path.path)) ?? []
            return contents.isEmpty ? nil : "\(path.path) exists, is not empty and is not an SVN working copy"
        }
        let info = try await svn.info(path.path)
        let normalize = { (s: String) in s.hasSuffix("/") ? String(s.dropLast()) : s }
        if normalize(info.url) != normalize(expected.absoluteString) {
            return "Working copy at \(path.path) is a checkout of \(info.url), not \(expected.absoluteString)"
        }
        return nil
    }

    /// Candidate files (other than `ignoring`) that are missing from, or differ from, the
    /// checked-out `releasedTree`, plus released files the candidate no longer has.
    static func differences(candidate: StagedCandidate, releasedTree: URL, ignoring: Set<String>) throws -> [String] {
        let fm = FileManager.default
        let root = releasedTree.standardizedFileURL.path + "/"
        var released: Set<String> = []
        if let enumerator = fm.enumerator(at: releasedTree, includingPropertiesForKeys: [.isRegularFileKey]) {
            while let url = enumerator.nextObject() as? URL {
                if url.lastPathComponent == ".svn" { enumerator.skipDescendants(); continue }
                guard try url.resourceValues(forKeys: [.isRegularFileKey]).isRegularFile == true else { continue }
                let full = url.standardizedFileURL.path
                if full.hasPrefix(root) { released.insert(String(full.dropFirst(root.count))) }
            }
        }
        var differences: [String] = []
        let files = candidate.entries.filter { $0.kind == .file && !ignoring.contains($0.path) }
        for entry in files {
            let file = releasedTree.appendingPathComponent(entry.path)
            if !released.contains(entry.path) || (try? Hashing.sha256(ofFile: file)) != entry.sha256 {
                differences.append(entry.path)
            }
        }
        differences += released.subtracting(files.map(\.path)).subtracting(ignoring)
        return differences.sorted()
    }

    /// Replaces `destination` with `source`, scheduling it for addition when it is new.
    static func replaceFile(at destination: URL, with source: URL, svn: SVNServicing) async throws {
        let fm = FileManager.default
        let existed = fm.fileExists(atPath: destination.path)
        if existed {
            if (try? Hashing.sha256(ofFile: destination)) == (try? Hashing.sha256(ofFile: source)) { return }
            try fm.removeItem(at: destination)
        }
        try fm.copyItem(at: source, to: destination)
        if !existed { try await svn.add([destination]) }
    }

    static func localChanges(at path: URL, svn: SVNServicing) async throws -> [String] {
        try await changes(at: path, svn: svn).map { "\($0.status.rawValue): \($0.path)" }
    }

    /// Local changes that make the working copy dirty, with paths relative to `path`.
    static func changes(at path: URL, svn: SVNServicing) async throws -> [WorkingCopyChange] {
        let root = path.standardizedFileURL.path
        return try await svn.status(path, showUpdates: false)
            .filter { entry in
                if entry.isLocalChange || entry.item == .unversioned { return true }
                // Ignored files would be overwritten or deleted by staging; only OS junk is exempt.
                guard entry.item == .ignored else { return false }
                let name = (entry.path as NSString).lastPathComponent
                return !FileClassifier.junkNames.contains(name) && !name.hasPrefix("._")
            }
            .map { entry in
                let p = entry.path.hasPrefix(root + "/") ? String(entry.path.dropFirst(root.count + 1)) : entry.path
                return WorkingCopyChange(path: p, status: entry.item, isConflict: entry.isConflict, isTreeConflict: entry.isTreeConflict)
            }
    }
}

/// Filesystem mirroring with content comparison. Never follows symlinks and never
/// touches `.svn` administrative directories.
enum FileMirror {
    /// Existing paths under `target` whose kind (file vs directory) differs in the candidate.
    /// Only the outermost path of each change is returned.
    static func typeChanges(entries: [PackageEntry], in target: URL) -> [URL] {
        let fm = FileManager.default
        var changes: [URL] = []
        for entry in entries.sorted(by: { $0.path.count < $1.path.count }) where entry.kind != .symlink {
            if changes.contains(where: { entry.path.hasPrefix($0.lastPathComponentPath(relativeTo: target) + "/") }) { continue }
            let url = target.appendingPathComponent(entry.path)
            var isDirectory: ObjCBool = false
            guard fm.fileExists(atPath: url.path, isDirectory: &isDirectory) else { continue }
            if isDirectory.boolValue != (entry.kind == .directory) { changes.append(url) }
        }
        return changes
    }

    static func mirror(source: URL, entries: [PackageEntry], into target: URL) throws {
        let fm = FileManager.default
        try fm.createDirectory(at: target, withIntermediateDirectories: true)
        let wantedFiles = Dictionary(uniqueKeysWithValues: entries.filter { $0.kind == .file }.map { ($0.path, $0) })
        let wantedDirs = Set(entries.filter { $0.kind == .directory }.map(\.path))

        // Remove what should not be there (deepest first).
        var existing: [(path: String, isDir: Bool)] = []
        let targetRoot = target.standardizedFileURL.path + "/"
        if let enumerator = fm.enumerator(at: target, includingPropertiesForKeys: [.isDirectoryKey, .isSymbolicLinkKey]) {
            while let url = enumerator.nextObject() as? URL {
                if url.lastPathComponent == ".svn" { enumerator.skipDescendants(); continue }
                let full = url.standardizedFileURL.path
                guard full.hasPrefix(targetRoot) else { continue }
                let values = try url.resourceValues(forKeys: [.isDirectoryKey, .isSymbolicLinkKey])
                existing.append((String(full.dropFirst(targetRoot.count)), values.isDirectory == true && values.isSymbolicLink != true))
            }
        }
        for item in existing.sorted(by: { $0.path.count > $1.path.count }) {
            let keep = item.isDir ? wantedDirs.contains(item.path) : wantedFiles[item.path] != nil
            if !keep {
                try fm.removeItem(at: target.appendingPathComponent(item.path))
            }
        }

        for dir in wantedDirs.sorted() {
            let url = try PathSafety.resolve(PathSafety.validateRelativePath(dir), under: target)
            try fm.createDirectory(at: url, withIntermediateDirectories: true)
        }
        for (path, entry) in wantedFiles.sorted(by: { $0.key < $1.key }) {
            let destination = try PathSafety.resolve(PathSafety.validateRelativePath(path), under: target)
            let sourceFile = source.appendingPathComponent(path)
            if fm.fileExists(atPath: destination.path), let sha = entry.sha256, (try? Hashing.sha256(ofFile: destination)) == sha {
                continue
            }
            try fm.createDirectory(at: destination.deletingLastPathComponent(), withIntermediateDirectories: true)
            if fm.fileExists(atPath: destination.path) { try fm.removeItem(at: destination) }
            try fm.copyItem(at: sourceFile, to: destination)
        }
    }
}

private extension URL {
    func lastPathComponentPath(relativeTo root: URL) -> String {
        let base = root.standardizedFileURL.path + "/"
        let full = standardizedFileURL.path
        return full.hasPrefix(base) ? String(full.dropFirst(base.count)) : full
    }
}
