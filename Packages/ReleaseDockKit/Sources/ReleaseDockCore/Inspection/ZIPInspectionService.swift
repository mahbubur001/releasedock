import Foundation

public protocol ZIPInspecting: Sendable {
    /// Inspects an archive without writing anything to disk.
    func inspect(zipAt url: URL, options: InspectionOptions) throws -> PackageInspection
    /// Safely extracts the included entries of an inspected archive into an empty
    /// `destination`, stripping the archive root folder. Returns entries with SHA-256.
    func extract(zipAt url: URL, inspection: PackageInspection, to destination: URL) throws -> [PackageEntry]
}

public enum PackageInspectionError: Error, LocalizedError, Sendable {
    case blockingIssues([InspectionIssue])
    case sourceChanged(String)
    case destinationNotEmpty(String)

    public var errorDescription: String? {
        switch self {
        case .blockingIssues(let issues):
            "The package has \(issues.count) blocking issue(s): " + issues.prefix(3).map { "\($0.path ?? "") \($0.message)" }.joined(separator: "; ")
        case .sourceChanged(let detail): "The source changed after it was inspected: \(detail)"
        case .destinationNotEmpty(let path): "Destination is not empty: \(path)"
        }
    }
}

public struct ZIPInspectionService: ZIPInspecting {
    public init() {}

    public func inspect(zipAt url: URL, options: InspectionOptions) throws -> PackageInspection {
        let sha = try Hashing.sha256(ofFile: url)
        let archive: ZIPArchive
        do {
            archive = try ZIPArchive(url: url)
        } catch let error as ZIPError {
            return PackageInspection(
                sourceKind: .zip, sourcePath: url.path, sha256: sha, archiveRootName: nil,
                includedEntries: [], excludedPaths: [],
                issues: [InspectionIssue(kind: .corruptArchive, severity: .error, path: nil, message: error.localizedDescription)],
                totalUncompressedSize: 0
            )
        }
        return analyze(entries: archive.entries, sourcePath: url.path, sha256: sha, options: options)
    }

    /// Pure analysis of central-directory entries; separated for testing.
    func analyze(entries: [ZIPEntry], sourcePath: String, sha256: String, options: InspectionOptions) -> PackageInspection {
        var issues: [InspectionIssue] = []
        var total: UInt64 = 0

        struct Candidate {
            var components: [String]
            var entry: ZIPEntry
        }
        var candidates: [Candidate] = []
        var seenFiles: [String: String] = [:] // lowercased path → original

        if entries.count > options.maximumEntries {
            issues.append(.init(kind: .archiveTooLarge, severity: .error, path: nil, message: "Archive has \(entries.count) entries (limit \(options.maximumEntries))"))
        }

        for entry in entries {
            guard let name = entry.name else {
                issues.append(.init(kind: .unsafeFilename, severity: .error, path: entry.rawName.base64EncodedString(), message: "File name is not valid UTF-8"))
                continue
            }
            let components: [String]
            do {
                components = try PathSafety.validateRelativePath(name)
            } catch {
                issues.append(.init(kind: error.issueKind, severity: .error, path: name, message: error.description))
                continue
            }
            if entry.isEncrypted {
                issues.append(.init(kind: .encryptedEntry, severity: .error, path: name, message: "Encrypted entries are not supported"))
                continue
            }
            if entry.isSymlink {
                issues.append(.init(kind: .symbolicLink, severity: .error, path: name, message: "Symbolic links are not allowed in a release package"))
                continue
            }
            if entry.isSpecialFile {
                issues.append(.init(kind: .specialFile, severity: .error, path: name, message: "Special files (devices, FIFOs, sockets) are not allowed"))
                continue
            }
            if !entry.isDirectory {
                if entry.method != 0 && entry.method != 8 {
                    issues.append(.init(kind: .unsupportedCompression, severity: .error, path: name, message: "Unsupported compression method \(entry.method)"))
                    continue
                }
                if entry.uncompressedSize > options.maximumFileSize {
                    issues.append(.init(kind: .archiveTooLarge, severity: .error, path: name, message: "File is larger than \(options.maximumFileSize >> 20) MiB"))
                }
                if entry.compressedSize > 0, entry.uncompressedSize > 1 << 20,
                   Double(entry.uncompressedSize) / Double(entry.compressedSize) > options.maximumCompressionRatio {
                    issues.append(.init(kind: .suspiciousCompressionRatio, severity: .error, path: name, message: "Suspicious compression ratio (possible zip bomb)"))
                }
                total += entry.uncompressedSize
                let key = components.joined(separator: "/").lowercased()
                if let existing = seenFiles[key] {
                    issues.append(.init(kind: .duplicatePath, severity: .error, path: name, message: "Duplicate archive path (also \"\(existing)\"; macOS file systems are case-insensitive)"))
                    continue
                }
                seenFiles[key] = name
            }
            candidates.append(Candidate(components: components, entry: entry))
        }

        if total > options.maximumTotalSize {
            issues.append(.init(kind: .archiveTooLarge, severity: .error, path: nil, message: "Uncompressed size \(total >> 20) MiB exceeds the \(options.maximumTotalSize >> 20) MiB limit"))
        }

        // A file path that is also used as a directory path is a conflict.
        let fileKeys = Set(seenFiles.keys)
        for candidate in candidates {
            for depth in 1..<max(candidate.components.count, 1) {
                let ancestor = candidate.components[..<depth].joined(separator: "/").lowercased()
                if fileKeys.contains(ancestor) {
                    issues.append(.init(kind: .duplicatePath, severity: .error, path: candidate.components.joined(separator: "/"), message: "Path uses \"\(ancestor)\" as both a file and a directory"))
                }
            }
        }

        // Determine the archive root, ignoring OS junk at the top level.
        let junkTopLevel: Set<String> = ["__MACOSX", ".DS_Store"]
        let meaningful = candidates.filter { !junkTopLevel.contains($0.components[0]) }
        let topLevel = Set(meaningful.map { $0.components[0] })
        var rootName: String?
        if topLevel.count == 1, let only = topLevel.first,
           meaningful.contains(where: { $0.components.count > 1 }),
           !meaningful.contains(where: { $0.components.count == 1 && !$0.entry.isDirectory }) {
            rootName = only
        } else if !meaningful.isEmpty {
            issues.append(.init(kind: .multipleRoots, severity: .warning, path: nil, message: "Archive does not wrap its files in a single top-level folder; the archive root is used as the package root"))
        }

        let classifier = FileClassifier(sourceKind: .zip, userPatterns: options.excludePatterns.compactMap(GlobPattern.init))
        var evaluator = PackagePathEvaluator(classifier: classifier)
        var included: [PackageEntry] = []
        var directories: Set<String> = []

        for candidate in candidates {
            var components = candidate.components
            if let rootName {
                if components[0] == rootName {
                    components.removeFirst()
                    if components.isEmpty { continue } // the root folder itself
                }
                // Entries outside the root (e.g. __MACOSX/) keep their path and are classified as junk.
            }
            let path = components.joined(separator: "/")
            let isDirectory = candidate.entry.isDirectory
            guard evaluator.include(path: path, isDirectory: isDirectory) else { continue }
            for depth in 1..<max(components.count, 1) {
                directories.insert(components[..<depth].joined(separator: "/"))
            }
            if isDirectory {
                directories.insert(path)
                continue
            }
            for component in components {
                if let warning = PathSafety.portabilityWarning(forComponent: component) {
                    evaluator.addIssue(.init(kind: .portability, severity: .warning, path: path, message: warning))
                    break
                }
            }
            if candidate.entry.uncompressedSize == 0 {
                evaluator.addIssue(.init(kind: .zeroByteFile, severity: .warning, path: path, message: "Zero-byte file"))
            }
            included.append(PackageEntry(path: path, kind: .file, size: candidate.entry.uncompressedSize))
        }

        // A package root that holds nothing but one folder means an extra nesting level
        // (e.g. my-plugin/my-plugin/my-plugin.php).
        let rootFiles = included.filter { !$0.path.contains("/") }
        let rootDirectories = directories.filter { !$0.contains("/") }
        if rootFiles.isEmpty, rootDirectories.count == 1, let nested = rootDirectories.first {
            evaluator.addIssue(.init(kind: .nestedRoot, severity: .error, path: nested + "/", message: "Unexpected nested directory: the package root contains only the folder \"\(nested)\""))
        }

        included += directories.sorted().map { PackageEntry(path: $0, kind: .directory, size: 0) }
        return PackageInspection(
            sourceKind: .zip,
            sourcePath: sourcePath,
            sha256: sha256,
            archiveRootName: rootName,
            includedEntries: included.sorted { $0.path < $1.path },
            excludedPaths: evaluator.excludedPaths,
            issues: issues + evaluator.issues,
            totalUncompressedSize: total
        )
    }

    public func extract(zipAt url: URL, inspection: PackageInspection, to destination: URL) throws -> [PackageEntry] {
        let blocking = inspection.blockingIssues
        guard blocking.isEmpty else { throw PackageInspectionError.blockingIssues(blocking) }
        guard try Hashing.sha256(ofFile: url) == inspection.sha256 else {
            throw PackageInspectionError.sourceChanged("ZIP checksum differs from the inspected archive")
        }
        let fm = FileManager.default
        try fm.createDirectory(at: destination, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
        guard try fm.contentsOfDirectory(atPath: destination.path).isEmpty else {
            throw PackageInspectionError.destinationNotEmpty(destination.path)
        }

        let archive = try ZIPArchive(url: url)
        var byPath: [String: ZIPEntry] = [:]
        for entry in archive.entries where !entry.isDirectory {
            guard let name = entry.name, var components = try? PathSafety.validateRelativePath(name) else { continue }
            if let root = inspection.archiveRootName {
                guard components.first == root else { continue }
                components.removeFirst()
            }
            byPath[components.joined(separator: "/")] = entry
        }

        var result: [PackageEntry] = []
        for item in inspection.includedEntries.sorted(by: { $0.path < $1.path }) {
            // Re-validate: never trust the stored inspection blindly.
            let components = try PathSafety.validateRelativePath(item.path)
            let target = try PathSafety.resolve(components, under: destination)
            switch item.kind {
            case .directory:
                try fm.createDirectory(at: target, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o755])
                result.append(item)
            case .file:
                guard let entry = byPath[item.path] else {
                    throw PackageInspectionError.sourceChanged("missing entry \(item.path)")
                }
                try fm.createDirectory(at: target.deletingLastPathComponent(), withIntermediateDirectories: true, attributes: [.posixPermissions: 0o755])
                // The parent must still resolve inside the destination (no symlinked parents).
                guard PathSafety.isContained(target.deletingLastPathComponent(), in: destination) else {
                    throw PathSafety.Violation.escapesRoot
                }
                let sha = try archive.extract(entry, to: target)
                var extracted = item
                extracted.sha256 = sha
                result.append(extracted)
            case .symlink:
                throw PackageInspectionError.blockingIssues([.init(kind: .symbolicLink, severity: .error, path: item.path, message: "Symbolic link")])
            }
        }
        return result
    }
}
