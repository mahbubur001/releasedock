import Foundation

/// WordPress.org theme releases. Themes have no trunk/tags: every version is its own
/// top-level directory (`<slug>/<version>/`). A new version is created by copying the
/// previous version directory (`svn cp 1.0.1 1.0.2`), replacing its contents, and
/// committing exactly one new version directory.
///
/// WordPress.org imports the commit *afterwards* and runs Theme Check; a failure is
/// emailed to the author and that version never goes live, while the directory stays
/// in SVN and cannot be overwritten. Validation is therefore deliberately strict.
public struct ThemeReleaseStrategy: ReleaseStrategy {
    public init() {}

    public var productType: ProductType { .theme }

    public func releasePath(version: String) -> String { version }

    public func allowedChangePrefixes(version: String) -> [String] { [version] }

    public func validateVersionFormat(_ version: String) -> [ValidationResult] {
        // Importer rule: ^\d+(\.\d+)*$, not all zeros, equal to the directory name.
        guard version.range(of: #"^\d+(\.\d+)*$"#, options: .regularExpression) != nil else {
            return [.fail(.versionValidity, "Theme versions must contain only numbers and dots (found \"\(version)\")", fix: "Use a version like 1.2.3.")]
        }
        if version.split(separator: ".").allSatisfy({ Int($0) == 0 }) {
            return [.fail(.versionValidity, "Theme version cannot be all zeros")]
        }
        return []
    }

    static let requiredHeaders = ["Theme Name", "Description", "Version", "Tags"]
    static let reviewHeaders = ["Author", "Requires at least", "Tested up to", "Requires PHP", "License", "License URI", "Text Domain"]

    public func validateMetadata(detected: DetectedProduct, product: Product) -> [ValidationResult] {
        var results: [ValidationResult] = []
        for header in Self.requiredHeaders where detected.headers[header] == nil {
            results.append(.fail(.metadataValidity, "style.css is missing the \"\(header)\" header, which the WordPress.org importer requires", file: "style.css"))
        }
        for header in Self.reviewHeaders where detected.headers[header] == nil {
            results.append(.warn(.metadataValidity, "style.css is missing the \"\(header)\" header required by theme review", file: "style.css"))
        }
        if let parent = detected.parentTheme {
            results.append(.init(check: .metadataValidity, status: .passed, severity: .info, message: "Child theme of \"\(parent)\""))
        }
        if let tested = detected.headers["Tested up to"], tested.range(of: #"^\d+\.\d(\.\d+)?$"#, options: .regularExpression) == nil {
            results.append(.warn(.testedUpTo, "Tested up to \"\(tested)\" is not a WordPress version number", file: "style.css"))
        }
        if let readme = detected.readme {
            for field in ["contributors", "requires at least", "tested up to", "requires php", "license", "license uri"] where readme.fields[field] == nil {
                results.append(.warn(.metadataValidity, "readme.txt is missing \"\(field.capitalized)\"", file: detected.readmeFile))
            }
            if product.validation.requireChangelogEntry, let version = detected.version, !readme.changelogMentions(version: version) {
                results.append(.warn(.changelogEntry, "readme.txt changelog has no entry for \(version)", file: detected.readmeFile))
            }
        }
        if results.allSatisfy({ $0.status == .passed }) {
            results.append(.pass(.metadataValidity, "style.css headers are complete"))
        }
        return results
    }

    public func validateRequiredFiles(inspection: PackageInspection, detected: DetectedProduct) -> [ValidationResult] {
        let files = Set(inspection.includedFiles.map(\.path))
        var results: [ValidationResult] = []
        if files.contains("style.css") {
            results.append(.pass(.requiredFiles, "style.css present"))
        } else {
            results.append(.fail(.requiredFiles, "style.css must be at the theme root"))
        }
        let isBlockTheme = files.contains("templates/index.html") || files.contains("block-templates/index.html")
        if files.contains("index.php") || isBlockTheme {
            results.append(.pass(.requiredFiles, isBlockTheme ? "Block theme templates/index.html present" : "index.php present"))
        } else {
            results.append(.fail(.requiredFiles, "A theme needs index.php (classic) or templates/index.html (block theme)"))
        }
        if isBlockTheme, !files.contains("theme.json") {
            results.append(.warn(.requiredFiles, "Block themes are required to include theme.json"))
        }
        if ["screenshot.png", "screenshot.jpg", "screenshot.jpeg", "screenshot.gif"].contains(where: files.contains) {
            results.append(.pass(.requiredFiles, "Screenshot present"))
        } else {
            results.append(.fail(.requiredFiles, "screenshot.png (or .jpg/.jpeg/.gif) is required at the theme root"))
        }
        if files.contains(where: { $0.lowercased() == "readme.txt" }) {
            results.append(.pass(.requiredFiles, "readme.txt present"))
        } else {
            results.append(.fail(.requiredFiles, "readme.txt is required for WordPress.org themes"))
        }
        return results
    }

    static let disallowedExtensions: Set<String> = ["sql", "sh", "dat", "wie"]
    static let allowedXML: Set<String> = ["wpml-config.xml", "loco.xml", "phpcs.xml"]

    public func validateForbiddenFiles(inspection: PackageInspection) -> [ValidationResult] {
        var results: [ValidationResult] = []
        for entry in inspection.includedEntries {
            let name = (entry.path as NSString).lastPathComponent
            let lower = name.lowercased()
            let ext = (lower as NSString).pathExtension
            var problem: String?
            if name.hasPrefix(".") {
                problem = "Hidden files are not allowed in WordPress.org themes"
            } else if lower == "favicon.ico" {
                problem = "favicon.ico is not allowed in WordPress.org themes"
            } else if entry.kind == .file, Self.disallowedExtensions.contains(ext) {
                problem = ".\(ext) files are not allowed in WordPress.org themes"
            } else if entry.kind == .file, ext == "xml", !Self.allowedXML.contains(lower) {
                problem = "XML files other than wpml-config.xml, loco.xml and phpcs.xml are not allowed in WordPress.org themes"
            }
            if let problem {
                results.append(.fail(.forbiddenFiles, problem, file: entry.path, fix: "Remove it or add an exclusion pattern. A rejected theme version cannot be re-committed."))
            }
        }
        return results
    }

    // MARK: Repository

    static func isVersionDirectory(_ name: String) -> Bool {
        name.first?.isNumber == true
    }

    public func inspectRemote(product: Product, candidateVersion: String?, svn: SVNServicing) async throws -> RepositorySnapshot {
        var snapshot = RepositorySnapshot()
        let base = product.svnRepositoryURL.absoluteString.trimmingTrailingSlash
        let info: SVNInfo
        do {
            info = try await svn.info(base)
        } catch let error as SVNError where error.kind == .notFound {
            snapshot.repositoryExists = false
            snapshot.structureProblems.append("The theme has no SVN directory. New themes must be submitted at https://wordpress.org/themes/upload/ and pass review first.")
            return snapshot
        }
        snapshot.repositoryExists = true
        snapshot.remoteRevision = info.lastChangedRevision ?? info.revision
        let entries = try await svn.list(base, recursive: false)
        let versions = entries.filter { $0.kind == .dir && Self.isVersionDirectory($0.name) }.map(\.name)
        snapshot.existingVersions = versions
        if entries.contains(where: { $0.name == "trunk" || $0.name == "tags" }) {
            snapshot.notes.append("The repository contains trunk/ or tags/; WordPress.org themes only import /<version>/ directories.")
        }
        if let highest = VersionService().highest(of: versions) {
            snapshot.currentVersion = highest
            snapshot.currentVersionSource = "highest version directory"
        } else {
            snapshot.structureProblems.append("No released version directory exists. The first version of a theme must go through the WordPress.org upload and review process.")
        }
        if let candidateVersion {
            snapshot.releasePathExists = versions.contains(candidateVersion)
        }
        return snapshot
    }

    public func prepareWorkingCopy(product: Product, currentVersion: String?, svn: SVNServicing) async throws {
        let wc = product.workingCopyURL
        let fm = FileManager.default
        if !fm.fileExists(atPath: wc.appendingPathComponent(".svn").path) {
            try fm.createDirectory(at: wc, withIntermediateDirectories: true)
            // Version directories are checked out empty; only the previous one is filled.
            try await svn.checkout(product.svnRepositoryURL.absoluteString, to: wc, depth: .immediates)
        } else {
            try await svn.update(wc, setDepth: nil)
        }
        if let currentVersion {
            try await svn.update(wc.appendingPathComponent(currentVersion), setDepth: .infinity)
        }
    }

    public func apply(candidate: StagedCandidate, version: String, currentVersion: String?, product: Product, svn: SVNServicing) async throws {
        let wc = product.workingCopyURL
        let target = wc.appendingPathComponent(version)
        if FileManager.default.fileExists(atPath: target.path) {
            throw ReleaseError.releasePathExists(version)
        }
        guard let currentVersion else {
            throw ReleaseError.invalidState("A theme release needs an existing version directory to copy from.")
        }
        // Copy the previous version with history, then replace its contents.
        try await svn.copy(wc.appendingPathComponent(currentVersion), to: target)
        try await WorkingCopyOperations.mirror(candidate: candidate, into: target, svn: svn)
    }

    public func verify(candidate: StagedCandidate, version: String, product: Product, svn: SVNServicing, scratch: URL) async throws -> VerificationReport {
        let base = product.svnRepositoryURL.absoluteString.trimmingTrailingSlash
        let url = "\(base)/\(version)"
        guard try await svn.exists(url) else {
            return VerificationReport(committedRevision: nil, tagExists: false, tagURL: url, notes: ["The version directory does not exist on the server."])
        }
        let info = try await svn.info(url)
        let tree = try await WorkingCopyOperations.verifyRemoteTree(url: url, candidate: candidate, svn: svn, scratch: scratch)
        return VerificationReport(
            committedRevision: info.lastChangedRevision,
            tagExists: true,
            tagURL: url,
            missingFiles: tree.missing,
            unexpectedFiles: tree.unexpected,
            checksumMismatches: tree.mismatched,
            notes: ["WordPress.org imports the version after the commit and runs Theme Check. Watch for a \"Theme Import Failure\" email; a failed version does not go live."]
        )
    }

    public func postReleaseGuidance(product: Product, version: String) -> [String] {
        [
            "WordPress.org now imports \(version) and runs Theme Check. Success is confirmed by email; a failure arrives as \"Theme Import Failure\" and the version does not go live.",
            "A committed version directory cannot be overwritten. Fixes require a new, higher version.",
        ]
    }
}
