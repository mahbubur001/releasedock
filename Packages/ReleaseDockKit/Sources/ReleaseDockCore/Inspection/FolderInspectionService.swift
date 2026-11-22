import Foundation

public protocol FolderInspecting: Sendable {
    /// Walks the folder read-only and hashes every included file.
    func inspect(folderAt url: URL, options: InspectionOptions) throws -> PackageInspection
}

public struct FolderInspectionService: FolderInspecting {
    public init() {}

    public func inspect(folderAt url: URL, options: InspectionOptions) throws -> PackageInspection {
        let fm = FileManager.default
        let root = url.standardizedFileURL
        var isDir: ObjCBool = false
        guard fm.fileExists(atPath: root.path, isDirectory: &isDir), isDir.boolValue else {
            throw ProductDetectionError.notADirectory(root.path)
        }

        var patterns = options.excludePatterns.compactMap(GlobPattern.init)
        var preIssues: [InspectionIssue] = []
        let distignore = root.appendingPathComponent(".distignore")
        if let text = try? String(contentsOf: distignore, encoding: .utf8) {
            let parsed = GlobPattern.parseList(text)
            patterns += parsed
            preIssues.append(.init(kind: .developmentArtifact, severity: .info, path: ".distignore", message: "Applied \(parsed.count) exclusion pattern(s) from .distignore"))
            if text.components(separatedBy: .newlines).contains(where: { $0.trimmingCharacters(in: .whitespaces).hasPrefix("!") }) {
                preIssues.append(.init(kind: .developmentArtifact, severity: .warning, path: ".distignore", message: "Negated patterns (!) in .distignore are not supported and were ignored"))
            }
        }

        var evaluator = PackagePathEvaluator(classifier: FileClassifier(sourceKind: .folder, userPatterns: patterns))
        for issue in preIssues { evaluator.addIssue(issue) }

        let keys: [URLResourceKey] = [.isDirectoryKey, .isRegularFileKey, .isSymbolicLinkKey, .fileSizeKey]
        guard let enumerator = fm.enumerator(at: root, includingPropertiesForKeys: keys, options: [], errorHandler: { _, _ in true }) else {
            throw CocoaError(.fileReadUnknown, userInfo: [NSFilePathErrorKey: root.path])
        }

        let rootPath = root.path.hasSuffix("/") ? root.path : root.path + "/"
        var included: [PackageEntry] = []
        var total: UInt64 = 0
        var count = 0

        while let item = enumerator.nextObject() as? URL {
            count += 1
            if count > options.maximumEntries {
                evaluator.addIssue(.init(kind: .archiveTooLarge, severity: .error, path: nil, message: "Folder has more than \(options.maximumEntries) entries"))
                break
            }
            let fullPath = item.standardizedFileURL.path
            guard fullPath.hasPrefix(rootPath) else { continue }
            let relative = String(fullPath.dropFirst(rootPath.count))

            do {
                _ = try PathSafety.validateRelativePath(relative)
            } catch {
                evaluator.addIssue(.init(kind: error.issueKind, severity: .error, path: relative, message: error.description))
                enumerator.skipDescendants()
                continue
            }

            let values = try item.resourceValues(forKeys: Set(keys))
            let isDirectory = values.isDirectory == true && values.isSymbolicLink != true
            guard evaluator.include(path: relative, isDirectory: isDirectory) else {
                if isDirectory { enumerator.skipDescendants() }
                continue
            }

            if values.isSymbolicLink == true {
                evaluator.addIssue(.init(kind: .symbolicLink, severity: .error, path: relative, message: "Symbolic links are not published; replace it with the real file or exclude it"))
                continue
            }
            if isDirectory {
                included.append(PackageEntry(path: relative, kind: .directory, size: 0))
                continue
            }
            guard values.isRegularFile == true else {
                evaluator.addIssue(.init(kind: .specialFile, severity: .error, path: relative, message: "Special files (devices, FIFOs, sockets) are not allowed"))
                continue
            }

            let size = UInt64(values.fileSize ?? 0)
            total += size
            if size > options.maximumFileSize {
                evaluator.addIssue(.init(kind: .archiveTooLarge, severity: .error, path: relative, message: "File is larger than \(options.maximumFileSize >> 20) MiB"))
                continue
            }
            if size == 0 {
                evaluator.addIssue(.init(kind: .zeroByteFile, severity: .warning, path: relative, message: "Zero-byte file"))
            }
            for component in relative.split(separator: "/") {
                if let warning = PathSafety.portabilityWarning(forComponent: String(component)) {
                    evaluator.addIssue(.init(kind: .portability, severity: .warning, path: relative, message: warning))
                    break
                }
            }
            let sha: String
            do {
                sha = try Hashing.sha256(ofFile: item)
            } catch {
                evaluator.addIssue(.init(kind: .unsafeFilename, severity: .error, path: relative, message: "File cannot be read: \(error.localizedDescription)"))
                continue
            }
            included.append(PackageEntry(path: relative, kind: .file, size: size, sha256: sha))
        }

        if total > options.maximumTotalSize {
            evaluator.addIssue(.init(kind: .archiveTooLarge, severity: .error, path: nil, message: "Folder size \(total >> 20) MiB exceeds the \(options.maximumTotalSize >> 20) MiB limit"))
        }

        // Case-insensitive duplicates cannot exist on a default macOS volume, but can on
        // case-sensitive volumes and would collide in SVN checkouts on macOS/Windows.
        var seen: [String: String] = [:]
        for entry in included {
            let key = entry.path.lowercased()
            if let other = seen[key] {
                evaluator.addIssue(.init(kind: .duplicatePath, severity: .error, path: entry.path, message: "Differs only by case from \"\(other)\""))
            }
            seen[key] = entry.path
        }

        let files = included.filter { $0.kind == .file }
        let digest = Hashing.treeDigest(files.map { ($0.path, $0.sha256 ?? "") })
        return PackageInspection(
            sourceKind: .folder,
            sourcePath: root.path,
            sha256: digest,
            archiveRootName: nil,
            includedEntries: included.sorted { $0.path < $1.path },
            excludedPaths: evaluator.excludedPaths,
            issues: evaluator.issues,
            totalUncompressedSize: total
        )
    }
}
